package com.jdtls.ecjbridge;

import org.eclipse.jdt.core.dom.*;

import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.*;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import org.eclipse.jdt.core.dom.IBinding;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.IVariableBinding;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * AST-based same-file hover/navigation/reference support.
 *
 * This intentionally does not depend on JDT search or bindings yet. It provides
 * useful bridge answers for declarations that can be resolved from the current
 * compilation unit alone.
 */
public class AstNavigationService {
    private static final Map<String, String> SOURCE_CACHE = new HashMap<>();

    private static final class ParsedUnit {
        final String uri;
        final String source;
        final CompilationUnit cu;
        final CompilationUnit bindingCu;

        ParsedUnit(String uri, String source, CompilationUnit cu, CompilationUnit bindingCu) {
            this.uri = uri;
            this.source = source;
            this.cu = cu;
            this.bindingCu = bindingCu;
        }
    }

    private static final class ExternalParsedUnit {
        final String source;
        final CompilationUnit cu;

        ExternalParsedUnit(String source, CompilationUnit cu) {
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

    private static final class BindingResolution {
        final Decl decl;
        final boolean attempted;

        BindingResolution(Decl decl, boolean attempted) {
            this.decl = decl;
            this.attempted = attempted;
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

    public String hover(Map<String, String> sourceFiles, String sourceLevel, List<String> classpath,
                        String targetUri, int offset) {
        // 1. Fast path: same-file AST lookup (no bindings, no classpath needed).
        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed != null) {
            // Don't show hover when the symbol at the cursor has an error problem.
            ASTNode hoverNode = nodeAt(parsed.cu, offset);
            SimpleName hoverName = hoverNode != null ? asSimpleName(hoverNode) : null;
            if (hoverName != null) {
                int hs = hoverName.getStartPosition(), he = hs + hoverName.getLength();
                for (org.eclipse.jdt.core.compiler.IProblem p : parsed.cu.getProblems()) {
                    if (p.isError() && p.getSourceStart() <= he && p.getSourceEnd() >= hs) {
                        return "";
                    }
                }
            }
            Decl decl = resolveDeclaration(parsed, offset);
            if (decl != null) {
                StringBuilder markdown = new StringBuilder();
                markdown.append("```java\n").append(renderSignature(parsed.source, decl)).append("\n```");
                String docs = renderDocumentation(decl);
                if (!docs.isBlank()) {
                    markdown.append("\n\n").append(docs);
                }
                return markdown.toString();
            }
        }

        // 2. Binding-based fallback: resolves JDK / library types via JDT bindings.
        return hoverWithBindings(sourceFiles, sourceLevel, classpath, targetUri, offset);
    }

    private String hoverWithBindings(Map<String, String> sourceFiles, String sourceLevel,
                                     List<String> classpath, String targetUri, int offset) {
        String source = sourceFiles.get(targetUri);
        if (source == null) return "";

        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(compilerOptions(sourceLevel));
        parser.setUnitName(unitName(targetUri));
        // Include the running VM's boot classpath so JDK types (System, String, …)
        // are always resolvable, plus any user-provided classpath entries.
        String[] cp = classpath != null ? classpath.toArray(new String[0]) : new String[0];
        parser.setEnvironment(cp, null, null, /* includeRunningVMBootclasspath */ true);

        CompilationUnit cu = (CompilationUnit) parser.createAST(null);

        ASTNode node = nodeAt(cu, offset);
        SimpleName name = asSimpleName(node);
        if (name == null) return "";

        // Suppress hover when the symbol has an error diagnostic (e.g. "cannot be resolved to a
        // type"). ECJ still resolves bindings via the running-VM boot classpath so `binding` would
        // be non-null, but showing Javadoc for an unresolvable reference is misleading.
        int nameStart = name.getStartPosition();
        int nameEnd   = nameStart + name.getLength();
        for (org.eclipse.jdt.core.compiler.IProblem p : cu.getProblems()) {
            if (p.isError() && p.getSourceStart() <= nameEnd && p.getSourceEnd() >= nameStart) {
                return "";
            }
        }

        IBinding binding = name.resolveBinding();
        if (binding == null) return "";
        String signature = renderBinding(binding);
        String documentation = renderExternalDocumentation(binding, sourceLevel);
        if (documentation.isBlank()) {
            return signature;
        }
        return signature + "\n\n" + documentation;
    }

    private String renderBinding(IBinding binding) {
        if (binding instanceof IVariableBinding var) return renderVariableBinding(var);
        if (binding instanceof IMethodBinding method) return renderMethodBinding(method);
        if (binding instanceof ITypeBinding type) return renderTypeBinding(type);
        return "";
    }

    private String renderVariableBinding(IVariableBinding var) {
        ITypeBinding type = var.getType();
        String typeName = type != null ? bindingSimpleName(type) : "?";
        StringBuilder sb = new StringBuilder("```java\n");
        if (var.isField()) {
            appendModifiers(sb, var.getModifiers());
            ITypeBinding declaring = var.getDeclaringClass();
            String className = declaring != null ? declaring.getName() + "." : "";
            sb.append(typeName).append(" ").append(className).append(var.getName());
        } else {
            sb.append(typeName).append(" ").append(var.getName());
        }
        return sb.append("\n```").toString();
    }

    private String renderMethodBinding(IMethodBinding method) {
        StringBuilder sb = new StringBuilder("```java\n");
        appendModifiers(sb, method.getModifiers());
        ITypeBinding declaring = method.getDeclaringClass();
        if (!method.isConstructor()) {
            ITypeBinding ret = method.getReturnType();
            sb.append(ret != null ? bindingSimpleName(ret) : "void").append(" ");
        }
        if (declaring != null) sb.append(declaring.getName()).append(".");
        sb.append(method.getName()).append("(");
        ITypeBinding[] params = method.getParameterTypes();
        for (int i = 0; i < params.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(bindingSimpleName(params[i]));
        }
        sb.append(")\n```");
        return sb.toString();
    }

    private String renderTypeBinding(ITypeBinding type) {
        StringBuilder sb = new StringBuilder("```java\n");
        appendModifiers(sb, type.getModifiers());
        String kind;
        if (type.isInterface()) kind = "interface";
        else if (type.isEnum()) kind = "enum";
        else if (type.isAnnotation()) kind = "@interface";
        else kind = "class";
        sb.append(kind).append(" ").append(type.getName());
        ITypeBinding superclass = type.getSuperclass();
        if (superclass != null && !"java.lang.Object".equals(superclass.getQualifiedName())) {
            sb.append(" extends ").append(superclass.getName());
        }
        ITypeBinding[] interfaces = type.getInterfaces();
        if (interfaces.length > 0) {
            sb.append(type.isInterface() ? " extends " : " implements ");
            for (int i = 0; i < interfaces.length; i++) {
                if (i > 0) sb.append(", ");
                sb.append(interfaces[i].getName());
            }
        }
        return sb.append("\n```").toString();
    }

    /** Simple (unqualified) name for a type binding, preserving generics and arrays. */
    private String bindingSimpleName(ITypeBinding type) {
        if (type == null) return "?";
        if (type.isArray()) {
            return bindingSimpleName(type.getElementType()) + "[]".repeat(type.getDimensions());
        }
        if (type.isParameterizedType()) {
            StringBuilder sb = new StringBuilder(type.getErasure().getName());
            ITypeBinding[] args = type.getTypeArguments();
            if (args.length > 0) {
                sb.append("<");
                for (int i = 0; i < args.length; i++) {
                    if (i > 0) sb.append(", ");
                    sb.append(bindingSimpleName(args[i]));
                }
                sb.append(">");
            }
            return sb.toString();
        }
        if (type.isWildcardType()) {
            ITypeBinding bound = type.getBound();
            if (bound == null) return "?";
            return type.isUpperbound() ? "? extends " + bindingSimpleName(bound)
                                       : "? super " + bindingSimpleName(bound);
        }
        return type.getName();
    }

    private void appendModifiers(StringBuilder sb, int mods) {
        if ((mods & org.eclipse.jdt.core.dom.Modifier.PUBLIC) != 0) sb.append("public ");
        else if ((mods & org.eclipse.jdt.core.dom.Modifier.PROTECTED) != 0) sb.append("protected ");
        else if ((mods & org.eclipse.jdt.core.dom.Modifier.PRIVATE) != 0) sb.append("private ");
        if ((mods & org.eclipse.jdt.core.dom.Modifier.STATIC) != 0) sb.append("static ");
        if ((mods & org.eclipse.jdt.core.dom.Modifier.FINAL) != 0) sb.append("final ");
        if ((mods & org.eclipse.jdt.core.dom.Modifier.ABSTRACT) != 0) sb.append("abstract ");
        if ((mods & org.eclipse.jdt.core.dom.Modifier.SYNCHRONIZED) != 0) sb.append("synchronized ");
    }

    public List<BridgeLocation> navigate(
            Map<String, String> sourceFiles,
            String sourceLevel,
            String targetUri,
            int offset,
            String kind) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        return switch (kind) {
            case "TypeDefinition"  -> navigateTypeDefinition(parsed, sourceFiles, sourceLevel, offset);
            case "Implementation"  -> navigateImplementation(parsed, sourceFiles, sourceLevel, offset);
            case "Definition", "Declaration" -> {
                Decl decl = resolveDeclaration(parsed, offset);
                if (decl != null) yield List.of(toLocation(parsed.uri, parsed.source, decl.nameNode));
                yield navigateCrossFile(parsed, sourceFiles, sourceLevel, offset);
            }
            default -> Collections.emptyList();
        };
    }

    /**
     * Cross-file fallback for goto-definition: searches every other open file for a
     * type/method/field declaration whose name matches the identifier at the cursor.
     * Priority: type declarations > methods > fields (to prefer the most specific match).
     */
    private List<BridgeLocation> navigateCrossFile(
            ParsedUnit parsed, Map<String, String> sourceFiles, String sourceLevel, int offset) {

        ASTNode node = nodeAt(parsed.cu, offset);
        SimpleName name = asSimpleName(node);
        if (name == null) return Collections.emptyList();
        String identifier = name.getIdentifier();

        List<BridgeLocation> typeMatches   = new ArrayList<>();
        List<BridgeLocation> methodMatches = new ArrayList<>();
        List<BridgeLocation> fieldMatches  = new ArrayList<>();

        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String uri = entry.getKey();
            if (uri.equals(parsed.uri)) continue;
            ParsedUnit other = parse(sourceFiles, sourceLevel, uri);
            if (other == null) continue;

            other.cu.accept(new ASTVisitor() {
                @Override public boolean visit(TypeDeclaration n) {
                    if (identifier.equals(n.getName().getIdentifier()))
                        typeMatches.add(toLocation(uri, other.source, n.getName()));
                    return true;
                }
                @Override public boolean visit(EnumDeclaration n) {
                    if (identifier.equals(n.getName().getIdentifier()))
                        typeMatches.add(toLocation(uri, other.source, n.getName()));
                    return true;
                }
                @Override public boolean visit(AnnotationTypeDeclaration n) {
                    if (identifier.equals(n.getName().getIdentifier()))
                        typeMatches.add(toLocation(uri, other.source, n.getName()));
                    return true;
                }
                @Override public boolean visit(MethodDeclaration n) {
                    if (!n.isConstructor() && identifier.equals(n.getName().getIdentifier()))
                        methodMatches.add(toLocation(uri, other.source, n.getName()));
                    return true;
                }
                @Override public boolean visit(VariableDeclarationFragment n) {
                    if (identifier.equals(n.getName().getIdentifier())
                            && n.getParent() instanceof FieldDeclaration)
                        fieldMatches.add(toLocation(uri, other.source, n.getName()));
                    return true;
                }
            });
        }

        if (!typeMatches.isEmpty())   return typeMatches;
        if (!methodMatches.isEmpty()) return methodMatches;
        return fieldMatches;
    }

    /**
     * Goto-type-definition: given a variable/parameter/field at the cursor, navigate
     * to the declaration of its declared type.
     */
    private List<BridgeLocation> navigateTypeDefinition(
            ParsedUnit parsed, Map<String, String> sourceFiles, String sourceLevel, int offset) {

        String typeName = resolveTypeName(parsed, offset);
        if (typeName == null) return Collections.emptyList();
        // Strip generic parameters (e.g. "List<String>" → "List")
        int lt = typeName.indexOf('<');
        if (lt >= 0) typeName = typeName.substring(0, lt).trim();
        int dot = typeName.lastIndexOf('.');
        if (dot >= 0) typeName = typeName.substring(dot + 1);
        if (typeName.isEmpty()) return Collections.emptyList();

        // Same file first
        Decl found = findTypeDeclaration(parsed.cu, typeName);
        if (found != null) return List.of(toLocation(parsed.uri, parsed.source, found.nameNode));

        // Cross-file search
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String uri = entry.getKey();
            if (uri.equals(parsed.uri)) continue;
            ParsedUnit other = parse(sourceFiles, sourceLevel, uri);
            if (other == null) continue;
            Decl otherFound = findTypeDeclaration(other.cu, typeName);
            if (otherFound != null) return List.of(toLocation(uri, other.source, otherFound.nameNode));
        }
        return Collections.emptyList();
    }

    /**
     * Extracts the declared-type name for the node at {@code offset}.
     * Handles: variable declarations, parameters, fields, method return types,
     * and plain type references (SimpleType / QualifiedName).
     */
    private String resolveTypeName(ParsedUnit parsed, int offset) {
        ASTNode node = nodeAt(parsed.cu, offset);
        ASTNode current = node;
        while (current != null) {
            if (current instanceof SimpleName sn) {
                ASTNode parent = sn.getParent();
                if (parent instanceof SimpleType) return sn.getIdentifier();
                if (parent instanceof SingleVariableDeclaration svd && svd.getName() == sn)
                    return svd.getType().toString();
                if (parent instanceof VariableDeclarationFragment vdf && vdf.getName() == sn) {
                    ASTNode gp = vdf.getParent();
                    if (gp instanceof VariableDeclarationStatement vds) return vds.getType().toString();
                    if (gp instanceof FieldDeclaration fd) return fd.getType().toString();
                }
                if (parent instanceof MethodDeclaration md && md.getName() == sn
                        && md.getReturnType2() != null)
                    return md.getReturnType2().toString();
            }
            current = current.getParent();
        }
        // Fallback: resolve declaration of the identifier, then get its type
        Decl decl = resolveDeclaration(parsed, offset);
        if (decl != null) {
            ASTNode dn = decl.declarationNode;
            if (dn instanceof SingleVariableDeclaration svd) return svd.getType().toString();
            if (dn instanceof VariableDeclarationFragment vdf) {
                ASTNode p = vdf.getParent();
                if (p instanceof VariableDeclarationStatement vds) return vds.getType().toString();
                if (p instanceof FieldDeclaration fd) return fd.getType().toString();
            }
        }
        return null;
    }

    /**
     * Goto-implementation: finds concrete overrides of the method/interface at the cursor
     * across all open source files.
     */
    private List<BridgeLocation> navigateImplementation(
            ParsedUnit parsed, Map<String, String> sourceFiles, String sourceLevel, int offset) {

        ASTNode node = nodeAt(parsed.cu, offset);
        SimpleName name = asSimpleName(node);
        if (name == null) return Collections.emptyList();

        // Determine the declaring type and method being referenced
        Decl decl = resolveDeclaration(parsed, offset);
        if (decl == null || (decl.kind != DeclKind.METHOD && decl.kind != DeclKind.TYPE))
            return Collections.emptyList();

        String methodName = decl.kind == DeclKind.METHOD
                ? decl.nameNode.getIdentifier() : null;
        String declaringTypeName = enclosingType(decl.declarationNode) != null
                ? enclosingType(decl.declarationNode).getName().getIdentifier()
                : (decl.kind == DeclKind.TYPE ? decl.nameNode.getIdentifier() : null);
        if (declaringTypeName == null) return Collections.emptyList();

        List<BridgeLocation> results = new ArrayList<>();

        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String uri = entry.getKey();
            ParsedUnit other = parse(sourceFiles, sourceLevel, uri);
            if (other == null) continue;

            other.cu.accept(new ASTVisitor() {
                @Override
                public boolean visit(TypeDeclaration td) {
                    // Check if this class extends/implements the declaring type
                    boolean isSubtype = false;
                    if (td.getSuperclassType() != null) {
                        String superName = simpleTypeName(td.getSuperclassType().toString());
                        if (declaringTypeName.equals(superName)) isSubtype = true;
                    }
                    for (Object iface : td.superInterfaceTypes()) {
                        if (declaringTypeName.equals(simpleTypeName(iface.toString()))) {
                            isSubtype = true;
                            break;
                        }
                    }
                    if (!isSubtype) return true;

                    // Find the overriding method (or report the class for type impls)
                    if (methodName != null) {
                        for (MethodDeclaration md : td.getMethods()) {
                            if (methodName.equals(md.getName().getIdentifier()) && !md.isConstructor()) {
                                results.add(toLocation(uri, other.source, md.getName()));
                            }
                        }
                    } else {
                        results.add(toLocation(uri, other.source, td.getName()));
                    }
                    return true;
                }
            });
        }
        return results;
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

        // Same-file references
        List<BridgeLocation> locations = new ArrayList<>(referenceLocations(parsed, decl));

        // Cross-file references: for types only (methods/fields are too noisy by name alone)
        if (decl.kind == DeclKind.TYPE || decl.kind == DeclKind.CONSTRUCTOR) {
            String identifier = decl.nameNode.getIdentifier();
            for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
                String uri = entry.getKey();
                if (uri.equals(parsed.uri)) continue;
                ParsedUnit other = parse(sourceFiles, sourceLevel, uri);
                if (other == null) continue;
                List<BridgeLocation> otherRefs = new ArrayList<>();
                other.cu.accept(new ASTVisitor() {
                    @Override
                    public boolean visit(SimpleName n) {
                        if (identifier.equals(n.getIdentifier()) && !(n.getParent() instanceof MethodDeclaration
                                && ((MethodDeclaration) n.getParent()).getName() == n)) {
                            otherRefs.add(toLocation(uri, other.source, n));
                        }
                        return true;
                    }
                });
                locations.addAll(otherRefs);
            }
        }

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
        int activeSignature = activeSignature(parsed, invocation, signatures, activeParameter);
        return new SignatureResult(signatures, activeSignature, activeParameter);
    }

    public List<BridgeInlayHint> inlayHints(
            Map<String, String> sourceFiles,
            List<String> classpath,
            String sourceLevel,
            String targetUri) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        // Build a param-name database from all open source files.
        // Key: "methodName/argCount", Value: ordered list of param names.
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
                        if (paramObj instanceof SingleVariableDeclaration svd)
                            names.add(svd.getName().getIdentifier());
                    }
                    paramDb.putIfAbsent(node.getName().getIdentifier() + "/" + names.size(), names);
                    return true;
                }
            });
        }

        // Parse with bindings + JVM boot classpath to resolve JDK method parameter names.
        String source = sourceFiles.get(targetUri);
        ASTParser bindingParser = ASTParser.newParser(AST.getJLSLatest());
        bindingParser.setSource(source.toCharArray());
        bindingParser.setKind(ASTParser.K_COMPILATION_UNIT);
        bindingParser.setResolveBindings(true);
        bindingParser.setBindingsRecovery(true);
        bindingParser.setStatementsRecovery(true);
        bindingParser.setCompilerOptions(compilerOptions(sourceLevel));
        bindingParser.setUnitName(unitName(targetUri));
        String[] cp = classpath != null ? classpath.toArray(new String[0]) : new String[0];
        bindingParser.setEnvironment(cp, null, null, false); // don't use running VM boot classpath (avoids hang on Java 25+)
        CompilationUnit bindingCu = (CompilationUnit) bindingParser.createAST(null);

        // Walk the target file for method / constructor invocations and emit hints.
        List<BridgeInlayHint> hints = new ArrayList<>();
        bindingCu.accept(new ASTVisitor() {
            private void emitHints(@SuppressWarnings("rawtypes") List rawArgs, List<String> params) {
                @SuppressWarnings("unchecked")
                List<Expression> args = (List<Expression>) rawArgs;
                if (args.isEmpty() || params == null) return;
                for (int i = 0; i < args.size() && i < params.size(); i++) {
                    Expression arg = args.get(i);
                    // Skip trivial single-name or string/number/boolean arguments — hints add noise there.
                    // CharacterLiteral and NullLiteral ARE shown (they're non-obvious to the reader).
                    if (arg instanceof SimpleName || arg instanceof StringLiteral
                            || arg instanceof NumberLiteral || arg instanceof BooleanLiteral)
                        continue;
                    int[] lc = CompilationService.offsetToLineCol(parsed.source, arg.getStartPosition());
                    BridgeInlayHint hint = new BridgeInlayHint();
                    hint.line = lc[0];
                    hint.character = lc[1];
                    hint.label = params.get(i) + ":";
                    hint.kind = 2; // Parameter
                    hints.add(hint);
                }
            }

            private List<String> resolveParams(String nameKey, IMethodBinding binding,
                                               @SuppressWarnings("rawtypes") List rawArgs) {
                // Prefer source-derived names (more reliable), fall back to binding.
                List<String> fromSource = paramDb.get(nameKey);
                if (fromSource != null) return fromSource;
                if (binding == null) return null;
                String[] names = binding.getParameterNames();
                if (names == null || names.length == 0) return null;
                return Arrays.asList(names);
            }

            @Override
            public boolean visit(MethodInvocation node) {
                String key = node.getName().getIdentifier() + "/" + node.arguments().size();
                emitHints(node.arguments(), resolveParams(key, node.resolveMethodBinding(), node.arguments()));
                return true;
            }

            @Override
            public boolean visit(SuperMethodInvocation node) {
                String key = node.getName().getIdentifier() + "/" + node.arguments().size();
                emitHints(node.arguments(), resolveParams(key, node.resolveMethodBinding(), node.arguments()));
                return true;
            }

            @Override
            public boolean visit(ClassInstanceCreation node) {
                Type t = node.getType();
                String name = t.isSimpleType()
                        ? ((SimpleType) t).getName().getFullyQualifiedName()
                        : t.toString();
                String key = name + "/" + node.arguments().size();
                emitHints(node.arguments(), resolveParams(key, node.resolveConstructorBinding(), node.arguments()));
                return true;
            }

            @Override
            public boolean visit(ConstructorInvocation node) {
                AbstractTypeDeclaration type = enclosingType(node);
                if (type != null) {
                    String key = type.getName().getIdentifier() + "/" + node.arguments().size();
                    emitHints(node.arguments(), resolveParams(key, node.resolveConstructorBinding(), node.arguments()));
                }
                return true;
            }
        });

        return hints;
    }

    public List<BridgeCodeLens> codeLens(Map<String, String> sourceFiles, String sourceLevel, String targetUri) {
        ParsedUnit parsed = parse(sourceFiles, sourceLevel, targetUri);
        if (parsed == null) return Collections.emptyList();

        List<BridgeCodeLens> lenses = new ArrayList<>();

        parsed.cu.accept(new ASTVisitor() {
            @Override
            public boolean visit(MethodDeclaration node) {
                SimpleName name = node.getName();
                Decl decl = withScope(new Decl(
                        node.isConstructor() ? DeclKind.CONSTRUCTOR : DeclKind.METHOD,
                        node,
                        name,
                        node));
                List<BridgeLocation> refs = referenceLocations(parsed, decl);
                int usages = Math.max(0, refs.size() - 1);
                BridgeCodeLens refLens = makeReferenceLens(parsed, name, refs, usages);
                lenses.add(refLens);

                // Run lens for public static void main(String[]) (TODO: Comment out run actions)
                // if (isMainMethod(node)) {
                //     BridgeCodeLens runLens = makeLens(parsed.source, name,
                //             "▶ Run", "jdtls-rust.run",
                //             List.of((Object) parsed.uri, String.valueOf(name.getStartPosition())));
                //     lenses.add(runLens);
                // }

                // // Test run lens for @Test annotated methods
                // if (hasAnnotation(node, "Test")) {
                //     BridgeCodeLens testLens = makeLens(parsed.source, name,
                //             "▶ Run Test", "jdtls-rust.runTest",
                //             List.of((Object) parsed.uri, name.getIdentifier()));
                //     lenses.add(testLens);
                // }

                return true;
            }

            @Override
            public boolean visit(TypeDeclaration node) {
                SimpleName name = node.getName();
                Decl decl = withScope(new Decl(DeclKind.TYPE, node, name, node));
                List<BridgeLocation> refs = referenceLocations(parsed, decl);
                int usages = Math.max(0, refs.size() - 1);
                lenses.add(makeReferenceLens(parsed, name, refs, usages));
                return true;
            }
        });

        return lenses;
    }

    // ── Call Hierarchy ────────────────────────────────────────────────────────

    public List<BridgeProtocol.BridgeCallHierarchyItem> prepareCallHierarchy(
            Map<String, String> sourceFiles, String sourceLevel, String uri, int offset) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        ASTNode node = nodeAt(parsed.cu, offset);
        MethodDeclaration md = enclosingMethod(node);
        if (md == null) return Collections.emptyList();

        return List.of(makeCallHierarchyItem(md, parsed.uri, parsed.source));
    }

    public List<BridgeProtocol.BridgeCallHierarchyIncomingCall> incomingCalls(
            Map<String, String> sourceFiles, String sourceLevel, String uri, int offset) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        ASTNode node = nodeAt(parsed.cu, offset);
        MethodDeclaration targetMethod = enclosingMethod(node);
        if (targetMethod == null) return Collections.emptyList();
        String targetName = targetMethod.getName().getIdentifier();
        boolean targetIsCtor = targetMethod.isConstructor();

        // Collect call sites grouped by their enclosing caller method, across all files
        Map<String, BridgeProtocol.BridgeCallHierarchyIncomingCall> result = new LinkedHashMap<>();

        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String fileUri = entry.getKey();
            ParsedUnit other = parse(sourceFiles, sourceLevel, fileUri);
            if (other == null) continue;

            Map<MethodDeclaration, List<BridgeProtocol.BridgeCallFromRange>> byMethod = new LinkedHashMap<>();

            other.cu.accept(new ASTVisitor() {
                private void record(String name, boolean isCtor, int start, int len) {
                    if (!targetName.equals(name) || targetIsCtor != isCtor) return;
                    MethodDeclaration caller = enclosingMethod(nodeAt(other.cu, start));
                    if (caller == null) return;
                    int[] s = CompilationService.offsetToLineCol(other.source, start);
                    int[] e = CompilationService.offsetToLineCol(other.source, start + len);
                    BridgeProtocol.BridgeCallFromRange r = new BridgeProtocol.BridgeCallFromRange();
                    r.startLine = s[0]; r.startChar = s[1]; r.endLine = e[0]; r.endChar = e[1];
                    byMethod.computeIfAbsent(caller, k -> new ArrayList<>()).add(r);
                }
                @Override public boolean visit(MethodInvocation n) {
                    record(n.getName().getIdentifier(), false, n.getStartPosition(), n.getLength());
                    return true;
                }
                @Override public boolean visit(ClassInstanceCreation n) {
                    if (targetIsCtor && n.getType() instanceof SimpleType st)
                        record(st.getName().getFullyQualifiedName(), true, n.getStartPosition(), n.getLength());
                    return true;
                }
            });

            for (Map.Entry<MethodDeclaration, List<BridgeProtocol.BridgeCallFromRange>> e2 : byMethod.entrySet()) {
                String key = fileUri + ":" + e2.getKey().getStartPosition();
                BridgeProtocol.BridgeCallHierarchyIncomingCall call = new BridgeProtocol.BridgeCallHierarchyIncomingCall();
                call.from = makeCallHierarchyItem(e2.getKey(), fileUri, other.source);
                call.fromRanges = e2.getValue();
                result.put(key, call);
            }
        }

        return new ArrayList<>(result.values());
    }

    public List<BridgeProtocol.BridgeCallHierarchyOutgoingCall> outgoingCalls(
            Map<String, String> sourceFiles, String sourceLevel, String uri, int offset) {

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        ASTNode node = nodeAt(parsed.cu, offset);
        MethodDeclaration callerMethod = enclosingMethod(node);
        if (callerMethod == null || callerMethod.getBody() == null) return Collections.emptyList();

        // Pre-index all method declarations across open files: name → [(uri, md, source)]
        Map<String, List<Object[]>> methodIndex = new HashMap<>();
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            String fileUri = entry.getKey();
            ParsedUnit other = parse(sourceFiles, sourceLevel, fileUri);
            if (other == null) continue;
            final String src = other.source;
            other.cu.accept(new ASTVisitor() {
                @Override public boolean visit(MethodDeclaration n) {
                    String key = (n.isConstructor() ? "new:" : "") + n.getName().getIdentifier();
                    methodIndex.computeIfAbsent(key, k -> new ArrayList<>())
                            .add(new Object[]{fileUri, n, src});
                    return true;
                }
            });
        }

        Map<String, BridgeProtocol.BridgeCallHierarchyOutgoingCall> result = new LinkedHashMap<>();

        callerMethod.getBody().accept(new ASTVisitor() {
            private void addOutgoing(String lookupKey, int callStart, int callLen) {
                List<Object[]> declarations = methodIndex.get(lookupKey);
                if (declarations == null || declarations.isEmpty()) return;
                Object[] first = declarations.get(0);
                String fileUri = (String) first[0];
                MethodDeclaration toMd = (MethodDeclaration) first[1];
                String src = (String) first[2];

                String key = fileUri + ":" + toMd.getStartPosition();
                BridgeProtocol.BridgeCallHierarchyOutgoingCall call = result.computeIfAbsent(key, k -> {
                    BridgeProtocol.BridgeCallHierarchyOutgoingCall c = new BridgeProtocol.BridgeCallHierarchyOutgoingCall();
                    c.to = makeCallHierarchyItem(toMd, fileUri, src);
                    c.fromRanges = new ArrayList<>();
                    return c;
                });
                int[] s = CompilationService.offsetToLineCol(parsed.source, callStart);
                int[] e = CompilationService.offsetToLineCol(parsed.source, callStart + callLen);
                BridgeProtocol.BridgeCallFromRange r = new BridgeProtocol.BridgeCallFromRange();
                r.startLine = s[0]; r.startChar = s[1]; r.endLine = e[0]; r.endChar = e[1];
                call.fromRanges.add(r);
            }

            @Override public boolean visit(MethodInvocation n) {
                addOutgoing(n.getName().getIdentifier(), n.getStartPosition(), n.getLength());
                return true;
            }
            @Override public boolean visit(ClassInstanceCreation n) {
                if (n.getType() instanceof SimpleType st)
                    addOutgoing("new:" + st.getName().getFullyQualifiedName(), n.getStartPosition(), n.getLength());
                return true;
            }
        });

        return new ArrayList<>(result.values());
    }

    // ── Type Hierarchy ────────────────────────────────────────────────────────

    public List<BridgeProtocol.BridgeTypeHierarchyItem> prepareTypeHierarchy(
            Map<String, String> sourceFiles, String sourceLevel, String uri, int offset) {
        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        ASTNode node = nodeAt(parsed.cu, offset);
        AbstractTypeDeclaration type = enclosingType(node);
        if (type == null) return Collections.emptyList();

        return List.of(makeTypeHierarchyItem(type, parsed.uri, parsed.source));
    }

    public List<BridgeProtocol.BridgeTypeHierarchyItem> supertypes(
            Map<String, String> sourceFiles, String sourceLevel, String data) {
        String[] parts = data.split("\t", 2);
        if (parts.length != 2) return Collections.emptyList();
        String uri = parts[0];
        int offset;
        try { offset = Integer.parseInt(parts[1]); } catch (NumberFormatException e) { return Collections.emptyList(); }

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        AbstractTypeDeclaration type = enclosingType(nodeAt(parsed.cu, offset));
        if (type == null) return Collections.emptyList();

        List<String> superNames = new ArrayList<>();
        if (type instanceof TypeDeclaration td) {
            if (td.getSuperclassType() != null) superNames.add(td.getSuperclassType().toString());
            for (Object iface : td.superInterfaceTypes()) superNames.add(iface.toString());
        } else if (type instanceof EnumDeclaration ed) {
            for (Object iface : ed.superInterfaceTypes()) superNames.add(iface.toString());
        }
        if (superNames.isEmpty()) return Collections.emptyList();

        List<BridgeProtocol.BridgeTypeHierarchyItem> results = new ArrayList<>();
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            ParsedUnit other = parse(sourceFiles, sourceLevel, entry.getKey());
            if (other == null) continue;
            other.cu.accept(new ASTVisitor() {
                @Override public boolean visit(TypeDeclaration n) {
                    if (superNames.contains(n.getName().getIdentifier()))
                        results.add(makeTypeHierarchyItem(n, other.uri, other.source));
                    return true;
                }
                @Override public boolean visit(EnumDeclaration n) {
                    if (superNames.contains(n.getName().getIdentifier()))
                        results.add(makeTypeHierarchyItem(n, other.uri, other.source));
                    return true;
                }
            });
        }
        return results;
    }

    public List<BridgeProtocol.BridgeTypeHierarchyItem> subtypes(
            Map<String, String> sourceFiles, String sourceLevel, String data) {
        String[] parts = data.split("\t", 2);
        if (parts.length != 2) return Collections.emptyList();
        String uri = parts[0];
        int offset;
        try { offset = Integer.parseInt(parts[1]); } catch (NumberFormatException e) { return Collections.emptyList(); }

        ParsedUnit parsed = parse(sourceFiles, sourceLevel, uri);
        if (parsed == null) return Collections.emptyList();

        AbstractTypeDeclaration type = enclosingType(nodeAt(parsed.cu, offset));
        if (type == null) return Collections.emptyList();
        String targetName = type.getName().getIdentifier();

        List<BridgeProtocol.BridgeTypeHierarchyItem> results = new ArrayList<>();
        for (Map.Entry<String, String> entry : sourceFiles.entrySet()) {
            ParsedUnit other = parse(sourceFiles, sourceLevel, entry.getKey());
            if (other == null) continue;
            other.cu.accept(new ASTVisitor() {
                @Override public boolean visit(TypeDeclaration n) {
                    if (extendsOrImplements(n, targetName))
                        results.add(makeTypeHierarchyItem(n, other.uri, other.source));
                    return true;
                }
                @Override public boolean visit(EnumDeclaration n) {
                    for (Object iface : n.superInterfaceTypes())
                        if (iface.toString().equals(targetName)) {
                            results.add(makeTypeHierarchyItem(n, other.uri, other.source));
                            break;
                        }
                    return true;
                }
            });
        }
        return results;
    }

    private boolean extendsOrImplements(TypeDeclaration n, String targetName) {
        if (n.getSuperclassType() != null && n.getSuperclassType().toString().equals(targetName)) return true;
        for (Object iface : n.superInterfaceTypes())
            if (iface.toString().equals(targetName)) return true;
        return false;
    }

    private BridgeProtocol.BridgeTypeHierarchyItem makeTypeHierarchyItem(
            AbstractTypeDeclaration type, String uri, String source) {
        BridgeProtocol.BridgeTypeHierarchyItem item = new BridgeProtocol.BridgeTypeHierarchyItem();
        item.name = type.getName().getIdentifier();
        item.kind = (type instanceof TypeDeclaration td && td.isInterface()) ? 11
                  : (type instanceof EnumDeclaration) ? 10
                  : 5; // 5=Class, 10=Enum, 11=Interface
        // detail = package name from compilation unit
        PackageDeclaration pkg = type.getRoot() instanceof CompilationUnit cu2 ? cu2.getPackage() : null;
        item.detail = pkg != null ? pkg.getName().getFullyQualifiedName() : null;
        item.uri = uri;
        int[] start = CompilationService.offsetToLineCol(source, type.getStartPosition());
        int[] end   = CompilationService.offsetToLineCol(source, type.getStartPosition() + type.getLength());
        item.startLine = start[0]; item.startChar = start[1];
        item.endLine = end[0];     item.endChar = end[1];
        int[] selS = CompilationService.offsetToLineCol(source, type.getName().getStartPosition());
        int[] selE = CompilationService.offsetToLineCol(source, type.getName().getStartPosition() + type.getName().getLength());
        item.selStartLine = selS[0]; item.selStartChar = selS[1];
        item.selEndLine = selE[0];   item.selEndChar = selE[1];
        // Opaque data: uri + tab + byte-offset of the type name node, for re-resolution
        item.data = uri + "\t" + type.getName().getStartPosition();
        return item;
    }

    private MethodDeclaration enclosingMethod(ASTNode node) {
        ASTNode current = node;
        while (current != null) {
            if (current instanceof MethodDeclaration md) return md;
            current = current.getParent();
        }
        return null;
    }

    private BridgeProtocol.BridgeCallHierarchyItem makeCallHierarchyItem(
            MethodDeclaration md, String uri, String source) {
        BridgeProtocol.BridgeCallHierarchyItem item = new BridgeProtocol.BridgeCallHierarchyItem();
        item.name = md.getName().getIdentifier();
        item.kind = md.isConstructor() ? 9 : 6; // LSP SymbolKind: 9=Constructor, 6=Method
        AbstractTypeDeclaration type = enclosingType(md);
        item.detail = type != null ? type.getName().getIdentifier() : null;
        item.uri = uri;
        int[] start = CompilationService.offsetToLineCol(source, md.getStartPosition());
        int[] end   = CompilationService.offsetToLineCol(source, md.getStartPosition() + md.getLength());
        item.startLine = start[0]; item.startChar = start[1];
        item.endLine = end[0];     item.endChar = end[1];
        int[] selS = CompilationService.offsetToLineCol(source, md.getName().getStartPosition());
        int[] selE = CompilationService.offsetToLineCol(source, md.getName().getStartPosition() + md.getName().getLength());
        item.selStartLine = selS[0]; item.selStartChar = selS[1];
        item.selEndLine = selE[0];   item.selEndChar = selE[1];
        return item;
    }

    private BridgeCodeLens makeReferenceLens(ParsedUnit parsed, SimpleName anchor,
                                             List<BridgeLocation> refs, int usageCount) {
        String title = usageCount + " reference" + (usageCount == 1 ? "" : "s");

        // Exclude the declaration itself: refs includes the declaration site, usageRefs does not.
        int[] anchorLC = CompilationService.offsetToLineCol(parsed.source, anchor.getStartPosition());
        List<BridgeLocation> usageRefs = refs.stream()
                .filter(r -> r.startLine != anchorLC[0] || r.startChar != anchorLC[1])
                .collect(java.util.stream.Collectors.toList());

        if (usageCount == 0) {
            // 0 references: clickable but leads nowhere (shows empty references panel).
            return makeLens(parsed.source, anchor, title,
                    "editor.action.showReferences",
                    buildShowReferencesArgs(parsed.uri, parsed.source, anchor, List.of()));
        }
        if (usageCount == 1) {
            // 1 reference: navigate directly without a popup.
            // editor.action.showReferences with a single location causes VS Code to navigate
            // directly (no peek panel) — same behaviour as jdtls uses for java.show.references.
            return makeLens(parsed.source, anchor, title,
                    "editor.action.showReferences",
                    buildShowReferencesArgs(parsed.uri, parsed.source, anchor, usageRefs));
        }
        // 2+ references: open the references panel.
        return makeLens(parsed.source, anchor, title,
                "editor.action.showReferences",
                buildShowReferencesArgs(parsed.uri, parsed.source, anchor, usageRefs));
    }

    private BridgeCodeLens makeLens(String source, SimpleName anchor, String title,
                                    String command, List<Object> args) {
        int[] lc = CompilationService.offsetToLineCol(source, anchor.getStartPosition());
        BridgeCodeLens lens = new BridgeCodeLens();
        lens.startLine = lc[0];
        lens.startChar = lc[1];
        int[] end = CompilationService.offsetToLineCol(source, anchor.getStartPosition() + anchor.getLength());
        lens.endLine = end[0];
        lens.endChar = end[1];
        lens.title = title;
        lens.command = command;
        lens.args = args;
        return lens;
    }

    private List<BridgeLocation> referenceLocations(ParsedUnit parsed, Decl decl) {
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

    private List<Object> buildShowReferencesArgs(String uri, String source, SimpleName anchor,
                                                 List<BridgeLocation> refs) {
        List<Object> args = new ArrayList<>();
        args.add(uriComponents(uri));
        args.add(positionValue(source, anchor.getStartPosition()));
        List<Object> locations = new ArrayList<>();
        for (BridgeLocation ref : refs) {
            locations.add(locationValue(ref));
        }
        args.add(locations);
        return args;
    }

    /** Args for editor.action.goToLocations: navigates directly for 1 location, picker for 2+. */
    private List<Object> buildGoToLocationsArgs(String uri, String source, SimpleName anchor,
                                                List<BridgeLocation> refs) {
        List<Object> args = new ArrayList<>();
        args.add(uriComponents(uri));
        args.add(positionValue(source, anchor.getStartPosition()));
        List<Object> locations = new ArrayList<>();
        for (BridgeLocation ref : refs) {
            locations.add(locationValue(ref));
        }
        args.add(locations);
        args.add("goto"); // multiple: always navigate to first/only location without a picker
        return args;
    }

    private Map<String, Object> uriComponents(String uri) {
        try {
            URI parsed = new URI(uri);
            Map<String, Object> value = new LinkedHashMap<>();
            value.put("$mid", 1);
            value.put("scheme", parsed.getScheme());
            value.put("authority", parsed.getRawAuthority() == null ? "" : parsed.getRawAuthority());
            value.put("path", parsed.getRawPath() == null ? "" : parsed.getRawPath());
            value.put("query", parsed.getRawQuery() == null ? "" : parsed.getRawQuery());
            value.put("fragment", parsed.getRawFragment() == null ? "" : parsed.getRawFragment());
            return value;
        } catch (Exception e) {
            Map<String, Object> fallback = new LinkedHashMap<>();
            fallback.put("$mid", 1);
            fallback.put("scheme", "file");
            fallback.put("authority", "");
            fallback.put("path", uri);
            fallback.put("query", "");
            fallback.put("fragment", "");
            return fallback;
        }
    }

    private Map<String, Object> positionValue(String source, int offset) {
        int[] lc = CompilationService.offsetToLineCol(source, offset);
        Map<String, Object> value = new LinkedHashMap<>();
        value.put("lineNumber", lc[0] + 1);
        value.put("column", lc[1] + 1);
        return value;
    }

    private Map<String, Object> locationValue(BridgeLocation ref) {
        Map<String, Object> value = new LinkedHashMap<>();
        value.put("uri", uriComponents(ref.uri));

        Map<String, Object> range = new LinkedHashMap<>();
        range.put("startLineNumber", ref.startLine + 1);
        range.put("startColumn", ref.startChar + 1);
        range.put("endLineNumber", ref.endLine + 1);
        range.put("endColumn", ref.endChar + 1);

        value.put("range", range);
        return value;
    }

    private boolean isMainMethod(MethodDeclaration node) {
        if (!"main".equals(node.getName().getIdentifier())) return false;
        if (node.parameters().size() != 1) return false;
        // Check public static void modifiers
        int mods = node.getModifiers();
        if ((mods & org.eclipse.jdt.core.dom.Modifier.PUBLIC) == 0) return false;
        if ((mods & org.eclipse.jdt.core.dom.Modifier.STATIC) == 0) return false;
        Type returnType = node.getReturnType2();
        if (returnType == null || !"void".equals(returnType.toString())) return false;
        // Check String[] parameter
        Object paramObj = node.parameters().get(0);
        if (!(paramObj instanceof SingleVariableDeclaration param)) return false;
        return param.getType().toString().contains("String");
    }

    private boolean hasAnnotation(MethodDeclaration node, String simpleName) {
        for (Object modObj : node.modifiers()) {
            if (modObj instanceof MarkerAnnotation ann
                    && simpleName.equals(ann.getTypeName().getFullyQualifiedName())) {
                return true;
            }
            if (modObj instanceof NormalAnnotation ann
                    && simpleName.equals(ann.getTypeName().getFullyQualifiedName())) {
                return true;
            }
        }
        return false;
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
        CompilationUnit bindingCu = parseWithBindings(source, sourceLevel, targetUri);
        return new ParsedUnit(targetUri, source, cu, bindingCu);
    }

    private CompilationUnit parseWithBindings(String source, String sourceLevel, String targetUri) {
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(true);
        parser.setCompilerOptions(compilerOptions(sourceLevel));
        parser.setUnitName(unitName(targetUri));
        parser.setEnvironment(new String[0], null, null, false);
        return (CompilationUnit) parser.createAST(null);
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
            BindingResolution binding = resolveTypeMemberWithBindings(parsed, name);
            if (binding.attempted) {
                return binding.decl != null ? withScope(binding.decl) : null;
            }

            Decl member = resolveTypeMember(enclosingType, name);
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
            ASTNode scope = localScope(parent);
            return new Decl(kind, parent, name, scope != null ? scope : parent);
        }
        if (parent instanceof VariableDeclarationFragment && location == VariableDeclarationFragment.NAME_PROPERTY) {
            ASTNode owner = parent.getParent();
            if (owner instanceof FieldDeclaration) {
                return new Decl(DeclKind.FIELD, parent, name, owner);
            }
            ASTNode scope = localScope(owner);
            return new Decl(DeclKind.LOCAL, parent, name, scope != null ? scope : owner);
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

    private Decl resolveTypeMember(AbstractTypeDeclaration type, SimpleName usage) {
        ASTNode parent = usage.getParent();

        if (parent instanceof MethodInvocation invocation && invocation.getName() == usage) {
            Expression expr = invocation.getExpression();
            if (expr != null && !(expr instanceof ThisExpression)) {
                return null;
            }
            return findMethodMember(type, usage.getIdentifier(), invocation.arguments().size());
        }

        if (parent instanceof SuperMethodInvocation invocation && invocation.getName() == usage) {
            return findMethodMember(type, usage.getIdentifier(), invocation.arguments().size());
        }

        if (parent instanceof FieldAccess access && access.getName() == usage) {
            Expression expr = access.getExpression();
            if (expr != null && !(expr instanceof ThisExpression)) {
                return null;
            }
            return findFieldMember(type, usage.getIdentifier());
        }

        return findTypeMember(type, usage.getIdentifier());
    }

    private BindingResolution resolveTypeMemberWithBindings(ParsedUnit parsed, SimpleName usage) {
        if (parsed.bindingCu == null) {
            return new BindingResolution(null, false);
        }

        ASTNode bindingNode = nodeAt(parsed.bindingCu, usage.getStartPosition());
        SimpleName bindingName = asSimpleName(bindingNode);
        if (bindingName == null
                || bindingName.getStartPosition() != usage.getStartPosition()
                || !usage.getIdentifier().equals(bindingName.getIdentifier())) {
            return new BindingResolution(null, false);
        }

        ASTNode parent = bindingName.getParent();
        if (parent instanceof MethodInvocation invocation && invocation.getName() == bindingName) {
            IMethodBinding binding = invocation.resolveMethodBinding();
            if (!matchesInvocationArity(binding, invocation.arguments().size())) {
                return new BindingResolution(null, true);
            }
            return resolveMethodBinding(parsed.bindingCu, binding);
        }
        if (parent instanceof SuperMethodInvocation invocation && invocation.getName() == bindingName) {
            IMethodBinding binding = invocation.resolveMethodBinding();
            if (!matchesInvocationArity(binding, invocation.arguments().size())) {
                return new BindingResolution(null, true);
            }
            return resolveMethodBinding(parsed.bindingCu, binding);
        }
        return new BindingResolution(null, false);
    }

    private boolean matchesInvocationArity(IMethodBinding binding, int argCount) {
        if (binding == null) {
            return false;
        }

        int paramCount = binding.getParameterTypes().length;
        if (binding.isVarargs()) {
            return argCount >= Math.max(0, paramCount - 1);
        }
        return argCount == paramCount;
    }

    private BindingResolution resolveMethodBinding(CompilationUnit cu, IMethodBinding binding) {
        if (binding == null) {
            return new BindingResolution(null, true);
        }

        AbstractTypeDeclaration type = findTypeDeclaration(cu, binding.getDeclaringClass());
        if (type == null) {
            return new BindingResolution(null, true);
        }

        ASTNode declaration = findMethodDeclaration(type, binding);
        if (declaration instanceof MethodDeclaration method) {
            DeclKind kind = method.isConstructor() ? DeclKind.CONSTRUCTOR : DeclKind.METHOD;
            return new BindingResolution(new Decl(kind, method, method.getName(), type), true);
        }

        return new BindingResolution(null, true);
    }

    private Decl findMethodMember(AbstractTypeDeclaration type, String identifier, int argCount) {
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof MethodDeclaration method
                    && identifier.equals(method.getName().getIdentifier())
                    && method.parameters().size() == argCount) {
                DeclKind kind = method.isConstructor() ? DeclKind.CONSTRUCTOR : DeclKind.METHOD;
                return new Decl(kind, method, method.getName(), type);
            }
        }
        return null;
    }

    private Decl findFieldMember(AbstractTypeDeclaration type, String identifier) {
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof FieldDeclaration field) {
                for (Object fragObj : field.fragments()) {
                    if (fragObj instanceof VariableDeclarationFragment frag
                            && identifier.equals(frag.getName().getIdentifier())) {
                        return new Decl(DeclKind.FIELD, frag, frag.getName(), type);
                    }
                }
            }
        }
        return null;
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

    private int activeSignature(
            ParsedUnit parsed,
            ASTNode invocation,
            List<BridgeSignature> signatures,
            int activeParameter) {

        Integer bindingIndex = activeSignatureFromBindings(parsed, invocation, signatures);
        if (bindingIndex != null) {
            return bindingIndex;
        }

        int requiredParams = activeParameter + 1;
        int bestIndex = -1;
        int bestParamCount = Integer.MAX_VALUE;
        for (int i = 0; i < signatures.size(); i++) {
            int paramCount = signatures.get(i).parameters != null ? signatures.get(i).parameters.size() : 0;
            if (paramCount >= requiredParams && paramCount < bestParamCount) {
                bestIndex = i;
                bestParamCount = paramCount;
            }
        }
        if (bestIndex >= 0) {
            return bestIndex;
        }

        return 0;
    }

    private Integer activeSignatureFromBindings(
            ParsedUnit parsed,
            ASTNode invocation,
            List<BridgeSignature> signatures) {

        if (parsed.bindingCu == null) {
            return null;
        }

        ASTNode bindingNode = nodeAt(parsed.bindingCu, invocation.getStartPosition());
        ASTNode bindingInvocation = enclosingInvocation(bindingNode);
        if (bindingInvocation == null
                || bindingInvocation.getStartPosition() != invocation.getStartPosition()) {
            return null;
        }

        IMethodBinding binding = resolveInvocationBinding(bindingInvocation);
        if (binding == null) {
            return null;
        }

        BindingResolution resolved = resolveMethodBinding(parsed.bindingCu, binding);
        if (resolved.decl == null || !(resolved.decl.declarationNode instanceof MethodDeclaration method)) {
            return null;
        }

        String targetLabel = renderMethodSignature(method, method.isConstructor());
        for (int i = 0; i < signatures.size(); i++) {
            if (targetLabel.equals(signatures.get(i).label)) {
                return i;
            }
        }
        return null;
    }

    private IMethodBinding resolveInvocationBinding(ASTNode invocation) {
        if (invocation instanceof MethodInvocation methodInvocation) {
            return methodInvocation.resolveMethodBinding();
        }
        if (invocation instanceof SuperMethodInvocation superMethodInvocation) {
            return superMethodInvocation.resolveMethodBinding();
        }
        if (invocation instanceof ClassInstanceCreation classInstanceCreation) {
            return classInstanceCreation.resolveConstructorBinding();
        }
        if (invocation instanceof ConstructorInvocation constructorInvocation) {
            return constructorInvocation.resolveConstructorBinding();
        }
        return null;
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

    private String renderExternalDocumentation(IBinding binding, String sourceLevel) {
        ExternalParsedUnit parsed = parseExternalSource(binding, sourceLevel);
        if (parsed == null) {
            return "";
        }

        ASTNode node = findExternalDeclaration(parsed.cu, binding);
        if (node instanceof MethodDeclaration method) {
            return renderJavadoc(method.getJavadoc());
        }
        if (node instanceof AbstractTypeDeclaration type) {
            return renderJavadoc(type.getJavadoc());
        }
        if (node instanceof FieldDeclaration field) {
            return renderJavadoc(field.getJavadoc());
        }
        if (node instanceof EnumConstantDeclaration constant) {
            return renderJavadoc(constant.getJavadoc());
        }
        return "";
    }

    private ExternalParsedUnit parseExternalSource(IBinding binding, String sourceLevel) {
        String source = readExternalSource(binding);
        if (source == null || source.isBlank()) {
            return null;
        }

        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        parser.setSource(source.toCharArray());
        parser.setKind(ASTParser.K_COMPILATION_UNIT);
        parser.setResolveBindings(false);
        parser.setStatementsRecovery(true);
        parser.setBindingsRecovery(true);
        parser.setCompilerOptions(compilerOptions(sourceLevel));
        CompilationUnit cu = (CompilationUnit) parser.createAST(null);
        return new ExternalParsedUnit(source, cu);
    }

    private String readExternalSource(IBinding binding) {
        ITypeBinding topLevelType = topLevelTypeForBinding(binding);
        if (topLevelType == null) {
            return null;
        }

        String path = sourcePath(topLevelType);
        if (path == null) {
            return null;
        }

        if (SOURCE_CACHE.containsKey(path)) {
            return SOURCE_CACHE.get(path);
        }

        String source = readSourceFromZip(path);
        SOURCE_CACHE.put(path, source);
        return source;
    }

    private ITypeBinding topLevelTypeForBinding(IBinding binding) {
        ITypeBinding type;
        if (binding instanceof ITypeBinding t) {
            type = t.getTypeDeclaration();
        } else if (binding instanceof IMethodBinding m) {
            type = m.getDeclaringClass();
        } else if (binding instanceof IVariableBinding v) {
            type = v.getDeclaringClass();
        } else {
            type = null;
        }
        if (type == null) {
            return null;
        }

        ITypeBinding current = type.getTypeDeclaration();
        while (current.getDeclaringClass() != null) {
            current = current.getDeclaringClass().getTypeDeclaration();
        }
        return current;
    }

    private String sourcePath(ITypeBinding topLevelType) {
        String packageName = topLevelType.getPackage() != null ? topLevelType.getPackage().getName() : "";
        String simpleName = topLevelType.getName();
        if (simpleName == null || simpleName.isBlank()) {
            return null;
        }
        String rel = simpleName + ".java";
        return packageName.isBlank() ? rel : packageName.replace('.', '/') + "/" + rel;
    }

    private String readSourceFromZip(String path) {
        for (Path zip : sourceZipCandidates()) {
            if (zip == null || !Files.isRegularFile(zip)) {
                continue;
            }

            try (ZipFile file = new ZipFile(zip.toFile())) {
                ZipEntry entry = file.getEntry(path);
                if (entry == null) {
                    Enumeration<? extends ZipEntry> entries = file.entries();
                    while (entries.hasMoreElements()) {
                        ZipEntry candidate = entries.nextElement();
                        String name = candidate.getName();
                        if (name.equals(path) || name.endsWith("/" + path)) {
                            entry = candidate;
                            break;
                        }
                    }
                }
                if (entry == null) {
                    continue;
                }
                return new String(file.getInputStream(entry).readAllBytes(), StandardCharsets.UTF_8);
            } catch (IOException ignored) {
                // Try the next candidate.
            }
        }
        return null;
    }

    private List<Path> sourceZipCandidates() {
        String javaHome = System.getProperty("java.home", "");
        if (javaHome.isBlank()) {
            return List.of();
        }

        Path home = Paths.get(javaHome);
        List<Path> candidates = new ArrayList<>();
        candidates.add(home.resolve("lib").resolve("src.zip"));
        candidates.add(home.resolve("src.zip"));
        if (home.getParent() != null) {
            candidates.add(home.getParent().resolve("src.zip"));
            candidates.add(home.getParent().resolve("lib").resolve("src.zip"));
        }
        if (home.getParent() != null && home.getParent().getParent() != null) {
            candidates.add(home.getParent().getParent().resolve("src.zip"));
        }
        return candidates;
    }

    private ASTNode findExternalDeclaration(CompilationUnit cu, IBinding binding) {
        if (binding instanceof ITypeBinding typeBinding) {
            return findTypeDeclaration(cu, typeBinding);
        }
        if (binding instanceof IMethodBinding methodBinding) {
            AbstractTypeDeclaration type = findTypeDeclaration(cu, methodBinding.getDeclaringClass());
            if (type == null) {
                return null;
            }
            return findMethodDeclaration(type, methodBinding);
        }
        if (binding instanceof IVariableBinding variableBinding) {
            if (variableBinding.isField() || variableBinding.isEnumConstant()) {
                AbstractTypeDeclaration type = findTypeDeclaration(cu, variableBinding.getDeclaringClass());
                if (type == null) {
                    return null;
                }
                return findFieldDeclaration(type, variableBinding);
            }
        }
        return null;
    }

    private AbstractTypeDeclaration findTypeDeclaration(CompilationUnit cu, ITypeBinding typeBinding) {
        List<String> names = typeNameChain(typeBinding);
        if (names.isEmpty()) {
            return null;
        }

        AbstractTypeDeclaration current = null;
        for (Object typeObj : cu.types()) {
            if (typeObj instanceof AbstractTypeDeclaration type
                    && names.get(0).equals(type.getName().getIdentifier())) {
                current = type;
                break;
            }
        }
        if (current == null) {
            return null;
        }

        for (int i = 1; i < names.size(); i++) {
            current = findNestedType(current, names.get(i));
            if (current == null) {
                return null;
            }
        }
        return current;
    }

    private List<String> typeNameChain(ITypeBinding typeBinding) {
        List<String> names = new ArrayList<>();
        ITypeBinding current = typeBinding != null ? typeBinding.getTypeDeclaration() : null;
        while (current != null) {
            String name = current.getName();
            if (name == null || name.isBlank()) {
                return List.of();
            }
            names.add(name);
            current = current.getDeclaringClass();
        }
        Collections.reverse(names);
        return names;
    }

    private AbstractTypeDeclaration findNestedType(AbstractTypeDeclaration type, String name) {
        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (bodyDeclObj instanceof AbstractTypeDeclaration nested
                    && name.equals(nested.getName().getIdentifier())) {
                return nested;
            }
        }
        return null;
    }

    private ASTNode findMethodDeclaration(AbstractTypeDeclaration type, IMethodBinding binding) {
        List<MethodDeclaration> candidates = new ArrayList<>();
        IMethodBinding target = binding.getMethodDeclaration();
        int paramCount = target.getParameterTypes().length;

        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (!(bodyDeclObj instanceof MethodDeclaration method)) continue;
            if (method.isConstructor() != target.isConstructor()) continue;
            if (!method.getName().getIdentifier().equals(target.getName())) continue;
            if (method.parameters().size() != paramCount) continue;
            candidates.add(method);
        }

        if (candidates.isEmpty()) {
            return null;
        }
        if (candidates.size() == 1) {
            return candidates.get(0);
        }

        for (MethodDeclaration candidate : candidates) {
            if (matchesMethodSignature(candidate, target)) {
                return candidate;
            }
        }
        return candidates.get(0);
    }

    private boolean matchesMethodSignature(MethodDeclaration method, IMethodBinding binding) {
        ITypeBinding[] params = binding.getParameterTypes();
        for (int i = 0; i < params.length; i++) {
            Object paramObj = method.parameters().get(i);
            if (!(paramObj instanceof SingleVariableDeclaration param)) {
                return false;
            }
            if (!typeMatches(param.getType(), param.isVarargs(), params[i])) {
                return false;
            }
        }
        return true;
    }

    private boolean typeMatches(Type sourceType, boolean varargs, ITypeBinding bindingType) {
        String source = normalizeTypeName(sourceType.toString(), varargs);
        if (source.isBlank()) {
            return false;
        }

        Set<String> candidates = new LinkedHashSet<>();
        collectBindingTypeNames(candidates, bindingType, varargs);
        return candidates.contains(source);
    }

    private void collectBindingTypeNames(Set<String> out, ITypeBinding bindingType, boolean varargs) {
        if (bindingType == null) {
            return;
        }
        ITypeBinding type = bindingType.getErasure();
        out.add(normalizeTypeName(type.getName(), varargs));
        out.add(normalizeTypeName(type.getQualifiedName(), varargs));
        out.add(normalizeTypeName(simpleTypeName(type.getQualifiedName()), varargs));
        if (type.isArray()) {
            ITypeBinding element = type.getElementType();
            if (element != null) {
                String suffix = "[]".repeat(type.getDimensions());
                if (varargs && suffix.endsWith("[]")) {
                    suffix = suffix.substring(0, suffix.length() - 2) + "...";
                }
                out.add(normalizeTypeName(element.getName() + suffix, false));
                out.add(normalizeTypeName(element.getQualifiedName() + suffix, false));
                out.add(normalizeTypeName(simpleTypeName(element.getQualifiedName()) + suffix, false));
            }
        }
    }

    private String normalizeTypeName(String value, boolean varargs) {
        String raw = stripGenericArguments(value == null ? "" : value).replace('$', '.').replace(" ", "");
        if (varargs) {
            raw = raw.replace("...", "[]");
        }
        return raw;
    }

    private String stripGenericArguments(String value) {
        StringBuilder out = new StringBuilder();
        int depth = 0;
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if (ch == '<') {
                depth++;
                continue;
            }
            if (ch == '>') {
                depth = Math.max(depth - 1, 0);
                continue;
            }
            if (depth == 0) {
                out.append(ch);
            }
        }
        return out.toString();
    }

    private String simpleTypeName(String qualifiedName) {
        int idx = qualifiedName.lastIndexOf('.');
        return idx >= 0 ? qualifiedName.substring(idx + 1) : qualifiedName;
    }

    private ASTNode findFieldDeclaration(AbstractTypeDeclaration type, IVariableBinding binding) {
        String name = binding.getName();
        if (binding.isEnumConstant() && type instanceof EnumDeclaration enumDecl) {
            for (Object constantObj : enumDecl.enumConstants()) {
                if (constantObj instanceof EnumConstantDeclaration constant
                        && name.equals(constant.getName().getIdentifier())) {
                    return constant;
                }
            }
        }

        for (Object bodyDeclObj : type.bodyDeclarations()) {
            if (!(bodyDeclObj instanceof FieldDeclaration field)) continue;
            for (Object fragObj : field.fragments()) {
                if (fragObj instanceof VariableDeclarationFragment fragment
                        && name.equals(fragment.getName().getIdentifier())) {
                    return field;
                }
            }
        }
        return null;
    }

    private String renderJavadoc(Javadoc javadoc) {
        if (javadoc == null) {
            return "";
        }

        List<String> summary = new ArrayList<>();
        List<String> params = new ArrayList<>();
        List<String> returns = new ArrayList<>();
        List<String> throwsDocs = new ArrayList<>();
        List<String> seeAlso = new ArrayList<>();
        List<String> since = new ArrayList<>();
        List<String> authors = new ArrayList<>();
        List<String> apiNotes = new ArrayList<>();
        List<String> implSpecs = new ArrayList<>();
        List<String> implNotes = new ArrayList<>();
        List<String> provides = new ArrayList<>();
        List<String> uses = new ArrayList<>();
        List<String> other = new ArrayList<>();

        for (Object tagObj : javadoc.tags()) {
            if (!(tagObj instanceof TagElement tag)) continue;

            String name = tag.getTagName();
            String rendered = renderTag(tag);
            if (rendered.isBlank()) {
                continue;
            }

            if (name == null) {
                summary.add(rendered);
            } else if ("@param".equals(name)) {
                params.add(formatNamedTagItem(tag));
            } else if ("@return".equals(name)) {
                returns.add(rendered);
            } else if ("@throws".equals(name) || "@exception".equals(name)) {
                throwsDocs.add(formatNamedTagItem(tag));
            } else if ("@see".equals(name)) {
                seeAlso.add(rendered);
            } else if ("@since".equals(name)) {
                since.add(rendered);
            } else if ("@author".equals(name)) {
                authors.add(rendered);
            } else if ("@apiNote".equals(name)) {
                apiNotes.add(rendered);
            } else if ("@implSpec".equals(name)) {
                implSpecs.add(rendered);
            } else if ("@implNote".equals(name)) {
                implNotes.add(rendered);
            } else if ("@provides".equals(name)) {
                provides.add(rendered);
            } else if ("@uses".equals(name)) {
                uses.add(rendered);
            } else {
                other.add("**" + name.substring(1) + ":**\n" + rendered);
            }
        }

        List<String> sections = new ArrayList<>();
        if (!summary.isEmpty()) {
            sections.add(String.join("\n\n", summary));
        }
        addSection(sections, "API Note", apiNotes, false);
        addSection(sections, "Implementation Note", implNotes, false);
        addSection(sections, "Implementation Requirements", implSpecs, false);
        if (!params.isEmpty()) {
            sections.add("**Parameters:**\n" + String.join("\n", params));
        }
        if (!returns.isEmpty()) {
            sections.add("**Returns:**\n" + String.join("\n\n", returns));
        }
        if (!throwsDocs.isEmpty()) {
            sections.add("**Throws:**\n" + String.join("\n", throwsDocs));
        }
        addSection(sections, "Provides", provides, true);
        addSection(sections, "Uses", uses, true);
        addSection(sections, "See", dedupePreserveOrder(seeAlso), true);
        addSection(sections, "Since", since, true);
        addSection(sections, "Author", authors, true);
        sections.addAll(other);

        return String.join("\n\n", sections).trim();
    }

    private void addSection(List<String> sections, String heading, List<String> items, boolean bullets) {
        if (items.isEmpty()) {
            return;
        }

        if (bullets) {
            sections.add("**" + heading + ":**\n- " + String.join("\n- ", items));
            return;
        }

        sections.add("**" + heading + ":**\n" + String.join("\n\n", items));
    }

    private List<String> dedupePreserveOrder(List<String> items) {
        return new ArrayList<>(new LinkedHashSet<>(items));
    }

    private String formatNamedTagItem(TagElement tag) {
        List<?> fragments = tag.fragments();
        if (fragments.isEmpty()) {
            return "-";
        }

        String label = normalizeWhitespace(renderFragment(fragments.get(0)));
        String body = fragments.size() > 1
            ? normalizeWhitespace(renderFragments(fragments.subList(1, fragments.size())))
            : "";

        if (label.isBlank()) {
            return body.isBlank() ? "-" : "- " + body;
        }
        if (body.isBlank()) {
            return "- `" + label + "`";
        }
        return "- `" + label + "`: " + body;
    }

    private String renderTag(TagElement tag) {
        String name = tag.getTagName();
        String fragments = normalizeWhitespace(renderFragments(tag.fragments()));
        if (name == null) {
            return fragments;
        }
        if ("@param".equals(name)) {
            return fragments;
        }
        if ("@return".equals(name)) {
            return fragments;
        }
        if ("@throws".equals(name) || "@exception".equals(name)) {
            return fragments;
        }
        if ("@see".equals(name)) {
            // Render as inline code unless it looks like a URL or a quoted string.
            if (!fragments.startsWith("\"") && !fragments.startsWith("<a ") && !fragments.startsWith("http")) {
                return "`" + fragments + "`";
            }
            return fragments;
        }
        if ("@since".equals(name)) {
            return fragments;
        }
        if ("@link".equals(name) || "@linkplain".equals(name) || "@code".equals(name)
                || "@literal".equals(name)) {
            return "`" + fragments + "`";
        }
        if ("@systemProperty".equals(name)) {
            return "`" + cleanSystemPropertyReference(fragments) + "`";
        }
        return "`" + name + "` " + fragments;
    }

    private String renderFragments(List<?> fragments) {
        StringBuilder out = new StringBuilder();
        for (Object fragment : fragments) {
            String rendered = renderFragment(fragment);
            if (rendered == null || rendered.isBlank()) {
                continue;
            }
            appendFragment(out, rendered);
        }
        return out.toString();
    }

    private String renderFragment(Object fragment) {
        if (fragment instanceof TextElement text) {
            return stripHtml(text.getText());
        }
        if (fragment instanceof Name name) {
            return name.getFullyQualifiedName();
        }
        if (fragment instanceof MemberRef memberRef) {
            return memberRef.getName().getIdentifier();
        }
        if (fragment instanceof MethodRef methodRef) {
            return methodRef.getName().getIdentifier();
        }
        if (fragment instanceof TagElement nested) {
            return renderInlineTag(nested);
        }
        return fragment != null ? fragment.toString() : "";
    }

    private String renderInlineTag(TagElement tag) {
        String name = tag.getTagName();
        String fragments = normalizeWhitespace(renderFragments(tag.fragments()));
        if (fragments.isBlank()) {
            return "";
        }
        if (name == null) {
            return fragments;
        }
        if ("@link".equals(name) || "@linkplain".equals(name) || "@code".equals(name)
                || "@literal".equals(name) || "@value".equals(name) || "@systemProperty".equals(name)) {
            if ("@systemProperty".equals(name)) {
                return "`" + cleanSystemPropertyReference(fragments) + "`";
            }
            return "`" + fragments + "`";
        }
        return fragments;
    }

    private void appendFragment(StringBuilder out, String fragment) {
        String trimmed = fragment.trim();
        if (trimmed.isEmpty()) {
            return;
        }

        boolean punctuation = ".,;:!?)]}".indexOf(trimmed.charAt(0)) >= 0;
        boolean opens = "([{" .indexOf(trimmed.charAt(0)) >= 0;
        if (out.length() > 0 && !Character.isWhitespace(out.charAt(out.length() - 1)) && !punctuation) {
            out.append(' ');
        }
        if (out.length() > 0 && opens && out.charAt(out.length() - 1) == ' ') {
            out.setLength(out.length() - 1);
        }
        out.append(trimmed);
    }

    private String normalizeWhitespace(String text) {
        Map<String, String> codeBlocks = new LinkedHashMap<>();
        Matcher matcher = Pattern.compile("```[\\s\\S]*?```").matcher(text);
        StringBuffer buffer = new StringBuffer();
        int index = 0;
        while (matcher.find()) {
            String token = "@@CODE_BLOCK_" + index++ + "@@";
            codeBlocks.put(token, normalizeCodeBlock(matcher.group()));
            matcher.appendReplacement(buffer, Matcher.quoteReplacement(token));
        }
        matcher.appendTail(buffer);

        // Preserve paragraph breaks (double newlines) before collapsing whitespace.
        // Single newlines (line-wrapping) are still collapsed to spaces.
        String PARA = "@@PARA@@";
        String raw = buffer.toString().replaceAll("\r?\n[ \t]*\r?\n", PARA);

        String normalized = raw
                .replace('\n', ' ')
                .replace('\r', ' ')
                .replaceAll("\\s+", " ")
                .replaceAll("\\s+([.,;:!?])", "$1")
                .replace("( ", "(")
                .replace(" )", ")")
                .trim()
                .replace(PARA, "\n\n");

        for (Map.Entry<String, String> entry : codeBlocks.entrySet()) {
            normalized = normalized.replace(entry.getKey(), entry.getValue());
        }
        return normalized;
    }

    private String normalizeCodeBlock(String codeBlock) {
        String body = codeBlock.substring(3, codeBlock.length() - 3).trim();
        if (body.startsWith("java")) {
            body = body.substring(4).trim();
            return "```java\n" + body + "\n```";
        }
        return "```\n" + body + "\n```";
    }

    private String cleanSystemPropertyReference(String value) {
        String cleaned = normalizeWhitespace(value).replaceFirst("^#+", "");
        int space = cleaned.indexOf(' ');
        if (space > 0) {
            String first = cleaned.substring(0, space);
            String rest = cleaned.substring(space + 1).trim();
            if (first.equals(rest)) {
                return first;
            }
        }
        return cleaned;
    }

    private String stripHtml(String text) {
        if (text == null || text.isBlank()) {
            return "";
        }

        return text
                .replaceAll("(?i)<pre>", "\n```java\n")
                .replaceAll("(?i)</pre>", "\n```\n")
                .replaceAll("(?i)<blockquote>", "\n")
                .replaceAll("(?i)</blockquote>", "\n")
                .replaceAll("(?i)<p>", "\n\n")
                .replaceAll("(?i)</p>", "")
                .replaceAll("(?i)<br\\s*/?>", "\n")
                .replaceAll("(?i)</?code>", "`")
                .replaceAll("(?i)<[^>]+>", " ");
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
