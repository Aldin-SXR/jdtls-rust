package com.jdtls.ecjbridge;

import org.eclipse.jdt.core.dom.*;

import java.net.URI;
import java.util.*;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * AST-based same-file hover/navigation/reference support.
 *
 * This intentionally does not depend on JDT search or bindings yet. It provides
 * useful bridge answers for declarations that can be resolved from the current
 * compilation unit alone.
 */
public class AstNavigationService {

    private static final class ParsedUnit {
        final String uri;
        final String source;
        final CompilationUnit cu;

        ParsedUnit(String uri, String source, CompilationUnit cu) {
            this.uri = uri;
            this.source = source;
            this.cu = cu;
        }
    }

    private enum DeclKind {
        LOCAL,
        PARAMETER,
        FIELD,
        METHOD,
        CONSTRUCTOR,
        TYPE,
        ENUM_CONSTANT
    }

    private static final class Decl {
        final DeclKind kind;
        final ASTNode declarationNode;
        final SimpleName nameNode;
        final ASTNode scopeNode;

        Decl(DeclKind kind, ASTNode declarationNode, SimpleName nameNode, ASTNode scopeNode) {
            this.kind = kind;
            this.declarationNode = declarationNode;
            this.nameNode = nameNode;
            this.scopeNode = scopeNode;
        }
    }

    public static final class SignatureResult {
        final List<BridgeSignature> signatures;
        final int activeSignature;
        final int activeParameter;

        SignatureResult(List<BridgeSignature> signatures, int activeSignature, int activeParameter) {
            this.signatures = signatures;
            this.activeSignature = activeSignature;
            this.activeParameter = activeParameter;
        }
    }

    public String hover(Map<String, String> sourceFiles, String sourceLevel, String targetUri, int offset) {
        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return "";

        Decl decl = resolveDeclaration(parsed, offset);
        if (decl == null) return "";

        StringBuilder markdown = new StringBuilder();
        markdown.append("```java\n").append(renderSignature(parsed.source, decl)).append("\n```");

        String docs = renderDocumentation(decl);
        if (!docs.isBlank()) {
            markdown.append("\n\n").append(docs);
        }

        return markdown.toString();
    }

    public List<BridgeLocation> navigate(
            Map<String, String> sourceFiles,
            String sourceLevel,
            String targetUri,
            int offset,
            String kind) {

        if (!"Definition".equals(kind) && !"Declaration".equals(kind)) {
            return Collections.emptyList();
        }

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        Decl decl = resolveDeclaration(parsed, offset);
        if (decl == null) return Collections.emptyList();

        return List.of(toLocation(parsed.uri, parsed.source, decl.nameNode));
    }

    public List<BridgeLocation> findReferences(
            Map<String, String> sourceFiles,
            String sourceLevel,
            String targetUri,
            int offset) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        Decl decl = resolveDeclaration(parsed, offset);
        if (decl == null) return Collections.emptyList();

        String identifier = decl.nameNode.getIdentifier();
        ASTNode scope = referenceScope(decl);
        List<BridgeLocation> locations = new ArrayList<>();
        Set<String> seen = new LinkedHashSet<>();

        scope.accept(new ASTVisitor() {
            @Override
            public boolean visit(SimpleName node) {
                if (!identifier.equals(node.getIdentifier())) {
                    return true;
                }
                Decl resolved = resolveDeclarationAtName(parsed, node);
                if (resolved == null || !sameDeclaration(resolved, decl)) {
                    return true;
                }

                BridgeLocation loc = toLocation(parsed.uri, parsed.source, node);
                String key = loc.startLine + ":" + loc.startChar + ":" + loc.endLine + ":" + loc.endChar;
                if (seen.add(key)) {
                    locations.add(loc);
                }
                return true;
            }
        });

        return locations;
    }

    public SignatureResult signatureHelp(
            Map<String, String> sourceFiles,
            String sourceLevel,
            String targetUri,
            int offset) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) {
            return new SignatureResult(Collections.emptyList(), 0, 0);
        }

        ASTNode invocation = enclosingInvocation(nodeAt(parsed.cu, offset));
        if (invocation == null) {
            return new SignatureResult(Collections.emptyList(), 0, 0);
        }

        List<BridgeSignature> signatures = signaturesForInvocation(parsed, invocation);
        if (signatures.isEmpty()) {
            return new SignatureResult(Collections.emptyList(), 0, 0);
        }

        int activeParameter = activeParameter(invocation, offset);
        return new SignatureResult(signatures, 0, activeParameter);
    }

    public List<BridgeInlayHint> inlayHints(
            Map<String, String> sourceFiles,
            String sourceLevel,
            String targetUri) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        // Build a param-name database from all open source files.
        // Key: "methodName/argCount", Value: ordered list of param names.
        // For constructors: "ClassName/argCount".
        Map<String, List<String>> paramDb = new HashMap<>();
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            ASTParser p = ASTParser.newParser(AST.getJLSLatest());
            p.setSource(entry.getValue().toCharArray());
            p.setKind(ASTParser.K_COMPILATION_UNIT);
            p.setResolveBindings(false);
            p.setStatementsRecovery(true);
            p.setCompilerOptions(compilerOptions(sourceLevel));
            CompilationUnit fileCu = (CompilationUnit) p.createAST(null);
            fileCu.accept(new ASTVisitor() {
                @Override
                public boolean visit(MethodDeclaration node) {
                    List<String> names = new ArrayList<>();
                    for (Object paramObj : node.parameters()) {
                        if (paramObj instanceof SingleVariableDeclaration svd) {
                            names.add(svd.getName().getIdentifier());
                        }
                    }
                    String key = node.getName().getIdentifier() + "/" + names.size();
                    paramDb.putIfAbsent(key, names);
                    return true;
                }
            });
        }

        // Walk the target file for method / constructor invocations and emit hints.
        List<BridgeInlayHint> hints = new ArrayList<>();
        parsed.cu.accept(new ASTVisitor() {
            private void addHints(String methodName, @SuppressWarnings("rawtypes") List rawArgs) {
                @SuppressWarnings("unchecked")
                List<Expression> args = (List<Expression>) rawArgs;
                if (args.isEmpty()) return;
                String key = methodName + "/" + args.size();
                List<String> params = paramDb.get(key);
                if (params == null) return;
                for (int i = 0; i < args.size() && i < params.size(); i++) {
                    Expression arg = args.get(i);
                    // Skip trivial single-name or literal arguments — hints add noise there.
                    if (arg instanceof SimpleName || arg instanceof StringLiteral
                            || arg instanceof NumberLiteral || arg instanceof BooleanLiteral
                            || arg instanceof NullLiteral || arg instanceof CharacterLiteral) {
                        continue;
                    }
                    int[] lc = CompilationService.offsetToLineCol(parsed.source, arg.getStartPosition());
                    BridgeInlayHint hint = new BridgeInlayHint();
                    hint.line = lc[0];
                    hint.character = lc[1];
                    hint.label = params.get(i) + ":";
                    hint.kind = 2; // Parameter
                    hints.add(hint);
                }
            }

            @Override
            public boolean visit(MethodInvocation node) {
                addHints(node.getName().getIdentifier(), node.arguments());
                return true;
            }

            @Override
            public boolean visit(SuperMethodInvocation node) {
                addHints(node.getName().getIdentifier(), node.arguments());
                return true;
            }

            @Override
            public boolean visit(ClassInstanceCreation node) {
                Type t = node.getType();
                String name = t.isSimpleType()
                        ? ((SimpleType) t).getName().getFullyQualifiedName()
                        : t.toString();
                addHints(name, node.arguments());
                return true;
            }

            @Override
            public boolean visit(ConstructorInvocation node) {
                AbstractTypeDeclaration type = enclosingType(node);
                if (type != null) {
                    addHints(type.getName().getIdentifier(), node.arguments());
                }
                return true;
            }
        });

        return hints;
    }

    private ParsedUnit parse(Map<String, String> sourceFiles, String sourceLevel, String targetUri) {
        String source = sourceFiles.get(targetUri);
        if (source == null) return null;

        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(false);
        parser.setStatementsRecovery(true);
        parser.setBindingsRecovery(true);
        parser.setCompilerOptions(compilerOptions(sourceLevel));
        parser.setUnitName(unitName(targetUri));

        CompilationUnit cu = (CompilationUnit) parser.createAST(null);
        return new ParsedUnit(targetUri, source, cu);
    }

    private Map<String, String> compilerOptions(String sourceLevel) {
        String ver = switch (sourceLevel.trim()) {
            case "8", "1.8" -> "1.8";
            case "11" -> "11";
            case "17" -> "17";
            case "21" -> "21";
            case "22" -> "22";
            default -> "21";
        };

        Map<String, String> opts = new HashMap<>();
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_SOURCE, ver);
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_COMPLIANCE, ver);
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_CODEGEN_TARGET_PLATFORM, ver);
        // Required so method.getJavadoc() returns the parsed Javadoc AST node
        opts.put(org.eclipse.jdt.core.JavaCore.COMPILER_DOC_COMMENT_SUPPORT,
                 org.eclipse.jdt.core.JavaCore.ENABLED);
        return opts;
    }

    private String unitName(String uri) {
        try {
            String path = new URI(uri).getPath();
            return path != null ? path : uri;
        } catch (Exception e) {
            return uri;
        }
    }

    private Decl resolveDeclaration(ParsedUnit parsed, int offset) {
        ASTNode node = nodeAt(parsed.cu, offset);
        SimpleName name = asSimpleName(node);
        if (name == null) return null;
        return resolveDeclarationAtName(parsed, name);
    }

    private Decl resolveDeclarationAtName(ParsedUnit parsed, SimpleName name) {
        Decl self = declarationFromName(name);
        if (self != null) {
            return withScope(self);
        }

        Decl local = resolveLocalDeclaration(parsed, name);
        if (local != null) {
            return local;
        }

        AbstractTypeDeclaration enclosingType = enclosingType(name);
        if (enclosingType != null) {
            Decl member = findTypeMember(enclosingType, name.getIdentifier());
            if (member != null) {
                return withScope(member);
            }
        }

        Decl type = findTypeDeclaration(parsed.cu, name.getIdentifier());
        return type != null ? withScope(type) : null;
    }

    private Decl resolveLocalDeclaration(ParsedUnit parsed, SimpleName usage) {
        Set<ASTNode> visibleScopes = visibleScopes(usage);
        List<Decl> candidates = new ArrayList<>();
        String identifier = usage.getIdentifier();
        int usageOffset = usage.getStartPosition();

        parsed.cu.accept(new ASTVisitor() {
            @Override
            public boolean visit(SingleVariableDeclaration node) {
                if (!identifier.equals(node.getName().getIdentifier())) {
                    return true;
                }
                if (node.getStartPosition() > usageOffset) {
                    return true;
                }
                ASTNode scope = localScope(node);
                if (scope != null && visibleScopes.contains(scope)) {
                    DeclKind kind = node.getParent() instanceof MethodDeclaration ? DeclKind.PARAMETER : DeclKind.LOCAL;
                    candidates.add(new Decl(kind, node, node.getName(), scope));
                }
                return true;
            }

            @Override
            public boolean visit(VariableDeclarationFragment node) {
                if (!identifier.equals(node.getName().getIdentifier())) {
                    return true;
                }
                if (node.getStartPosition() > usageOffset) {
                    return true;
                }
                ASTNode parent = node.getParent();
                if (parent instanceof VariableDeclarationStatement || parent instanceof VariableDeclarationExpression) {
                    ASTNode scope = localScope(parent);
                    if (scope != null && visibleScopes.contains(scope)) {
                        candidates.add(new Decl(DeclKind.LOCAL, node, node.getName(), scope));
                    }
                }
                return true;
            }
        });

        Decl best = null;
        for (Decl candidate : candidates) {
            if (best == null || candidate.nameNode.getStartPosition() >= best.nameNode.getStartPosition()) {
                best = candidate;
            }
        }
        return best;
    }

    private Decl declarationFromName(SimpleName name) {
        ASTNode parent = name.getParent();
        StructuralPropertyDescriptor location = name.getLocationInParent();

        if (parent instanceof MethodDeclaration && location == MethodDeclaration.NAME_PROPERTY) {
            MethodDeclaration method = (MethodDeclaration) parent;
            DeclKind kind = method.isConstructor() ? DeclKind.CONSTRUCTOR : DeclKind.METHOD;
            return new Decl(kind, parent, name, parent);
        }
        if (parent instanceof TypeDeclaration && location == TypeDeclaration.NAME_PROPERTY) {
            return new Decl(DeclKind.TYPE, parent, name, parent);
        }
        if (parent instanceof EnumDeclaration && location == EnumDeclaration.NAME_PROPERTY) {
            return new Decl(DeclKind.TYPE, parent, name, parent);
        }
        if (parent instanceof RecordDeclaration && location == RecordDeclaration.NAME_PROPERTY) {
            return new Decl(DeclKind.TYPE, parent, name, parent);
        }
        if (parent instanceof AnnotationTypeDeclaration && location == AnnotationTypeDeclaration.NAME_PROPERTY) {
            return new Decl(DeclKind.TYPE, parent, name, parent);
        }
        if (parent instanceof EnumConstantDeclaration && location == EnumConstantDeclaration.NAME_PROPERTY) {
            return new Decl(DeclKind.ENUM_CONSTANT, parent, name, parent);
        }
        if (parent instanceof SingleVariableDeclaration && location == SingleVariableDeclaration.NAME_PROPERTY) {
            DeclKind kind = parent.getParent() instanceof MethodDeclaration ? DeclKind.PARAMETER : DeclKind.LOCAL;
            return new Decl(kind, parent, name, parent);
        }
        if (parent instanceof VariableDeclarationFragment && location == VariableDeclarationFragment.NAME_PROPERTY) {
            ASTNode owner = parent.getParent();
            if (owner instanceof FieldDeclaration) {
                return new Decl(DeclKind.FIELD, parent, name, owner);
            }
            return new Decl(DeclKind.LOCAL, parent, name, owner);
        }
        return null;
    }

    private Decl withScope(Decl decl) {
        ASTNode scope = switch (decl.kind) {
            case LOCAL, PARAMETER -> decl.scopeNode;
            case FIELD, METHOD, CONSTRUCTOR, ENUM_CONSTANT -> enclosingType(decl.nameNode);
            case TYPE -> decl.declarationNode.getRoot();
        };
        return new Decl(decl.kind, decl.declarationNode, decl.nameNode, scope != null ? scope : decl.scopeNode);
    }

    private Decl findTypeMember(AbstractTypeDeclaration type, String identifier) {
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof FieldDeclaration field) {
                for (Object fragObj : field.fragments()) {
                    if (fragObj instanceof VariableDeclarationFragment frag
                            && identifier.equals(frag.getName().getIdentifier())) {
                        return new Decl(DeclKind.FIELD, frag, frag.getName(), type);
                    }
                }
            } else if (bodyDeclObj instanceof MethodDeclaration method) {
                if (identifier.equals(method.getName().getIdentifier())) {
                    DeclKind kind = method.isConstructor() ? DeclKind.CONSTRUCTOR : DeclKind.METHOD;
                    return new Decl(kind, method, method.getName(), type);
                }
            } else if (bodyDeclObj instanceof AbstractTypeDeclaration nested) {
                SimpleName name = nested.getName();
                if (name != null && identifier.equals(name.getIdentifier())) {
                    return new Decl(DeclKind.TYPE, nested, name, nested);
                }
            }
        }

        if (type instanceof EnumDeclaration enumDecl) {
            for (Object constantObj : enumDecl.enumConstants()) {
                if (constantObj instanceof EnumConstantDeclaration constant
                        && identifier.equals(constant.getName().getIdentifier())) {
                    return new Decl(DeclKind.ENUM_CONSTANT, constant, constant.getName(), type);
                }
            }
        }

        return null;
    }

    private Decl findTypeDeclaration(CompilationUnit cu, String identifier) {
        for (Object typeObj : cu.types()) {
            if (typeObj instanceof AbstractTypeDeclaration type) {
                Decl found = findTypeDeclaration(type, identifier);
                if (found != null) {
                    return found;
                }
            }
        }
        return null;
    }

    private Decl findTypeDeclaration(AbstractTypeDeclaration type, String identifier) {
        SimpleName name = type.getName();
        if (name != null && identifier.equals(name.getIdentifier())) {
            return new Decl(DeclKind.TYPE, type, name, type);
        }

        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof AbstractTypeDeclaration nested) {
                Decl found = findTypeDeclaration(nested, identifier);
                if (found != null) {
                    return found;
                }
            }
        }
        return null;
    }

    private List<BridgeSignature> signaturesForInvocation(ParsedUnit parsed, ASTNode invocation) {
        if (invocation instanceof MethodInvocation methodInvocation) {
            AbstractTypeDeclaration type = enclosingType(methodInvocation);
            if (type == null) return Collections.emptyList();
            String name = methodInvocation.getName().getIdentifier();
            return methodSignatures(type, name, false);
        }

        if (invocation instanceof SuperMethodInvocation superMethodInvocation) {
            AbstractTypeDeclaration type = enclosingType(superMethodInvocation);
            if (type == null) return Collections.emptyList();
            String name = superMethodInvocation.getName().getIdentifier();
            return methodSignatures(type, name, false);
        }

        if (invocation instanceof ClassInstanceCreation classInstanceCreation) {
            Type typeNode = classInstanceCreation.getType();
            if (typeNode.isSimpleType()) {
                Name name = ((SimpleType) typeNode).getName();
                String identifier = name.getFullyQualifiedName();
                Decl decl = findTypeDeclaration(parsed.cu, identifier);
                if (decl != null && decl.declarationNode instanceof AbstractTypeDeclaration type) {
                    return methodSignatures(type, identifier, true);
                }
            }
        }

        if (invocation instanceof ConstructorInvocation constructorInvocation) {
            AbstractTypeDeclaration type = enclosingType(constructorInvocation);
            if (type == null) return Collections.emptyList();
            return methodSignatures(type, type.getName().getIdentifier(), true);
        }

        return Collections.emptyList();
    }

    private List<BridgeSignature> methodSignatures(AbstractTypeDeclaration type, String name, boolean constructorsOnly) {
        List<BridgeSignature> signatures = new ArrayList<>();
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (!(bodyDeclObj instanceof MethodDeclaration method)) continue;
            if (constructorsOnly != method.isConstructor()) continue;
            if (!name.equals(method.getName().getIdentifier())) continue;

            BridgeSignature signature = new BridgeSignature();
            signature.label = renderMethodSignature(method, method.isConstructor());
            signature.documentation = renderJavadoc(method.getJavadoc());
            signature.parameters = new ArrayList<>();
            for (Object paramObj : method.parameters()) {
                if (!(paramObj instanceof SingleVariableDeclaration param)) continue;
                BridgeParameter bridgeParam = new BridgeParameter();
                bridgeParam.label = param.getType() + " " + param.getName().getIdentifier();
                bridgeParam.documentation = renderParamDocumentation(method.getJavadoc(), param.getName().getIdentifier());
                signature.parameters.add(bridgeParam);
            }
            signatures.add(signature);
        }
        return signatures;
    }

    private ASTNode referenceScope(Decl decl) {
        return decl.scopeNode != null ? decl.scopeNode : decl.declarationNode.getRoot();
    }

    private boolean sameDeclaration(Decl left, Decl right) {
        return left.kind == right.kind
                && left.nameNode.getStartPosition() == right.nameNode.getStartPosition()
                && left.nameNode.getLength() == right.nameNode.getLength();
    }

    private ASTNode nodeAt(CompilationUnit cu, int offset) {
        CompletionService.NodeLocator locator = new CompletionService.NodeLocator(offset);
        cu.accept(locator);
        return locator.found;
    }

    private ASTNode enclosingInvocation(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof MethodInvocation
                    || current instanceof SuperMethodInvocation
                    || current instanceof ClassInstanceCreation
                    || current instanceof ConstructorInvocation) {
                return current;
            }
            current = current.getParent();
        }
        return null;
    }

    private SimpleName asSimpleName(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof SimpleName simpleName) {
                return simpleName;
            }
            current = current.getParent();
        }
        return null;
    }

    private Set<ASTNode> visibleScopes(SimpleName usage) {
        Set<ASTNode> scopes = new HashSet<>();
        ASTNode current = usage;
        while (current != null) {
            if (isScope(current)) {
                scopes.add(current);
            }
            current = current.getParent();
        }
        return scopes;
    }

    private ASTNode localScope(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (isScope(current)) {
                return current;
            }
            current = current.getParent();
        }
        return null;
    }

    private boolean isScope(ASTNode node) {
        return node instanceof MethodDeclaration
                || node instanceof Initializer
                || node instanceof Block
                || node instanceof CatchClause
                || node instanceof ForStatement
                || node instanceof EnhancedForStatement
                || node instanceof LambdaExpression;
    }

    private int activeParameter(ASTNode invocation, int offset) {
        @SuppressWarnings("unchecked")
        List<Expression> arguments =
                invocation instanceof MethodInvocation methodInvocation ? methodInvocation.arguments()
                : invocation instanceof SuperMethodInvocation superMethodInvocation ? superMethodInvocation.arguments()
                : invocation instanceof ClassInstanceCreation classInstanceCreation ? classInstanceCreation.arguments()
                : invocation instanceof ConstructorInvocation constructorInvocation ? constructorInvocation.arguments()
                : Collections.emptyList();

        // Count arguments that are fully before the cursor to find the active index.
        // Using end position avoids the off-by-one that start position would cause.
        int active = 0;
        for (Expression argument : arguments) {
            int argEnd = argument.getStartPosition() + argument.getLength();
            if (argEnd < offset) {
                active++;
            }
        }
        if (!arguments.isEmpty()) {
            active = Math.min(active, arguments.size() - 1);
        }
        return Math.max(active, 0);
    }

    private AbstractTypeDeclaration enclosingType(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof AbstractTypeDeclaration type) {
                return type;
            }
            current = current.getParent();
        }
        return null;
    }

    private String renderSignature(String source, Decl decl) {
        return switch (decl.kind) {
            case FIELD, LOCAL -> renderVariableSignature(source, decl);
            case PARAMETER -> renderParameterSignature(source, decl);
            case METHOD -> renderMethodSignature((MethodDeclaration) decl.declarationNode, false);
            case CONSTRUCTOR -> renderMethodSignature((MethodDeclaration) decl.declarationNode, true);
            case TYPE -> renderTypeSignature((AbstractTypeDeclaration) decl.declarationNode);
            case ENUM_CONSTANT -> decl.nameNode.getIdentifier();
        };
    }

    private String renderDocumentation(Decl decl) {
        ASTNode node = decl.declarationNode;

        if (node instanceof MethodDeclaration method) {
            return renderJavadoc(method.getJavadoc());
        }
        if (node instanceof AbstractTypeDeclaration type) {
            return renderJavadoc(type.getJavadoc());
        }
        if (node instanceof VariableDeclarationFragment fragment && fragment.getParent() instanceof FieldDeclaration field) {
            return renderJavadoc(field.getJavadoc());
        }
        if (node instanceof EnumConstantDeclaration constant) {
            return renderJavadoc(constant.getJavadoc());
        }

        return "";
    }

    private String renderJavadoc(Javadoc javadoc) {
        if (javadoc == null) {
            return "";
        }

        StringBuilder out = new StringBuilder();
        for (Object tagObj : javadoc.tags()) {
            if (!(tagObj instanceof TagElement tag)) continue;
            String rendered = renderTag(tag);
            if (!rendered.isBlank()) {
                if (out.length() > 0) {
                    out.append("\n");
                }
                out.append(rendered);
            }
        }
        return out.toString().trim();
    }

    private String renderTag(TagElement tag) {
        String name = tag.getTagName();
        String fragments = renderFragments(tag.fragments()).trim();
        if (name == null) {
            return fragments;
        }
        if ("@param".equals(name)) {
            return "*param* " + fragments;
        }
        if ("@return".equals(name)) {
            return "*returns* " + fragments;
        }
        if ("@throws".equals(name) || "@exception".equals(name)) {
            return "*throws* " + fragments;
        }
        return "`" + name + "` " + fragments;
    }

    private String renderFragments(List<?> fragments) {
        StringBuilder out = new StringBuilder();
        for (Object fragment : fragments) {
            String rendered;
            if (fragment instanceof TextElement text) {
                rendered = text.getText();
            } else if (fragment instanceof Name name) {
                rendered = name.getFullyQualifiedName();
            } else if (fragment instanceof MemberRef memberRef) {
                rendered = memberRef.getName().getIdentifier();
            } else if (fragment instanceof MethodRef methodRef) {
                rendered = methodRef.getName().getIdentifier();
            } else if (fragment instanceof TagElement nested) {
                rendered = renderTag(nested);
            } else {
                rendered = fragment.toString();
            }

            if (rendered == null || rendered.isBlank()) {
                continue;
            }
            if (out.length() > 0 && !Character.isWhitespace(out.charAt(out.length() - 1))) {
                out.append(" ");
            }
            out.append(rendered.trim());
        }
        return out.toString();
    }

    private String renderParamDocumentation(Javadoc javadoc, String paramName) {
        if (javadoc == null) {
            return null;
        }

        for (Object tagObj : javadoc.tags()) {
            if (!(tagObj instanceof TagElement tag)) continue;
            if (!"@param".equals(tag.getTagName())) continue;
            List<?> fragments = tag.fragments();
            if (fragments.isEmpty()) continue;
            Object first = fragments.get(0);
            if (first instanceof SimpleName name && paramName.equals(name.getIdentifier())) {
                return renderFragments(fragments.subList(1, fragments.size())).trim();
            }
        }

        return null;
    }

    private String renderVariableSignature(String source, Decl decl) {
        ASTNode parent = decl.declarationNode.getParent();
        if (parent instanceof FieldDeclaration field) {
            return field.getType() + " " + decl.nameNode.getIdentifier();
        }
        if (parent instanceof VariableDeclarationStatement stmt) {
            return stmt.getType() + " " + decl.nameNode.getIdentifier();
        }
        if (parent instanceof VariableDeclarationExpression expr) {
            return expr.getType() + " " + decl.nameNode.getIdentifier();
        }
        return decl.nameNode.getIdentifier();
    }

    private String renderParameterSignature(String source, Decl decl) {
        if (decl.declarationNode instanceof SingleVariableDeclaration svd) {
            return svd.getType() + " " + svd.getName().getIdentifier();
        }
        return decl.nameNode.getIdentifier();
    }

    private String renderMethodSignature(MethodDeclaration method, boolean constructor) {
        StringBuilder sb = new StringBuilder();
        if (!constructor && method.getReturnType2() != null) {
            sb.append(method.getReturnType2()).append(" ");
        }
        sb.append(method.getName().getIdentifier()).append("(");
        boolean first = true;
        for (Object paramObj : method.parameters()) {
            if (!(paramObj instanceof SingleVariableDeclaration param)) continue;
            if (!first) sb.append(", ");
            first = false;
            sb.append(param.getType()).append(" ").append(param.getName().getIdentifier());
        }
        sb.append(")");
        return sb.toString();
    }

    private String renderTypeSignature(AbstractTypeDeclaration type) {
        String kind;
        if (type instanceof TypeDeclaration td) {
            kind = td.isInterface() ? "interface" : "class";
        } else if (type instanceof EnumDeclaration) {
            kind = "enum";
        } else if (type instanceof RecordDeclaration) {
            kind = "record";
        } else if (type instanceof AnnotationTypeDeclaration) {
            kind = "@interface";
        } else {
            kind = "type";
        }
        return kind + " " + type.getName().getIdentifier();
    }

    private BridgeLocation toLocation(String uri, String source, SimpleName node) {
        int[] start = CompilationService.offsetToLineCol(source, node.getStartPosition());
        int[] end = CompilationService.offsetToLineCol(source, node.getStartPosition() + node.getLength());

        BridgeLocation loc = new BridgeLocation();
        loc.uri = uri;
        loc.startLine = start[0];
        loc.startChar = start[1];
        loc.endLine = end[0];
        loc.endChar = end[1];
        return loc;
    }
}
