package com.jdtls.ecjbridge;

import java.lang.reflect.Constructor;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.jdt.core.IJavaProject;
import org.eclipse.jdt.core.IPackageFragmentRoot;
import org.eclipse.jdt.core.compiler.CharOperation;
import org.eclipse.jdt.core.search.IJavaSearchConstants;
import org.eclipse.jdt.internal.codeassist.ISearchRequestor;
import org.eclipse.jdt.internal.compiler.ExtraFlags;
import org.eclipse.jdt.internal.compiler.classfmt.ClassFileConstants;
import org.eclipse.jdt.internal.compiler.env.ICompilationUnit;
import org.eclipse.jdt.internal.compiler.env.IModule;
import org.eclipse.jdt.internal.compiler.env.IUpdatableModule;
import org.eclipse.jdt.internal.compiler.env.IUpdatableModule.UpdateKind;
import org.eclipse.jdt.internal.compiler.env.NameEnvironmentAnswer;
import org.eclipse.jdt.internal.compiler.lookup.ExtraCompilerModifiers;
import org.eclipse.jdt.internal.compiler.lookup.ModuleBinding;
import org.eclipse.jdt.internal.core.SearchableEnvironment;

/**
 * A {@link SearchableEnvironment} for JDT's CompletionEngine that needs no
 * Java model: types come from in-memory sources, the classpath and the JDK,
 * and type-name search is answered by {@link TypeIndex}.
 *
 * <p>SearchableEnvironment's constructors require a JavaProject, so instances
 * are allocated without running them (see {@link #create}) and every method
 * the engine uses is overridden here.  The environment behaves like the
 * pre-Java-9 (classpath) mode of SearchableEnvironment: every package lives in
 * the unnamed module.
 */
public class CodeAssistEnvironment extends SearchableEnvironment {

    private TypeIndex index;
    private InMemoryNameEnvironment binaries;
    private Map<String, String> files;
    private String skipUri;
    private Set<String> testUris;
    private boolean excludeTestCode;

    /** Never run: instances are allocated by {@link #create} without a constructor call. */
    private CodeAssistEnvironment() throws org.eclipse.jdt.core.JavaModelException {
        super((org.eclipse.jdt.internal.core.JavaProject) null, (org.eclipse.jdt.core.WorkingCopyOwner) null, false, 0);
    }

    @SuppressWarnings("restriction")
    static CodeAssistEnvironment create(Map<String, String> files, Set<String> testUris, List<String> classpath,
            String sourceLevel, String skipUri, boolean excludeTestCode) {
        CodeAssistEnvironment env;
        try {
            Constructor<?> ctor = sun.reflect.ReflectionFactory.getReflectionFactory()
                    .newConstructorForSerialization(CodeAssistEnvironment.class, Object.class.getDeclaredConstructor());
            env = (CodeAssistEnvironment) ctor.newInstance();
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Cannot allocate code assist environment", e);
        }
        env.files = files;
        env.skipUri = skipUri;
        env.testUris = testUris;
        env.excludeTestCode = excludeTestCode;
        env.index = new TypeIndex(files, testUris, classpath, sourceLevel, skipUri, excludeTestCode);
        env.binaries = new InMemoryNameEnvironment(Map.of(), classpath);
        return env;
    }

    TypeIndex index() {
        return index;
    }

    // ── Type lookup ──────────────────────────────────────────────────────────

    private NameEnvironmentAnswer find(char[][] compoundName) {
        if (compoundName == null || compoundName.length == 0) return null;
        String fqn = CharOperation.toString(compoundName);
        String uri = index.sourceTypeUris.get(fqn);
        if (uri != null) {
            if (uri.equals(skipUri)) return null;
            return new NameEnvironmentAnswer(new InMemoryCompilationUnit(uri, files.get(uri)), null);
        }
        return binaries.findType(compoundName);
    }

    @Override
    public NameEnvironmentAnswer findType(char[][] compoundTypeName, char[] moduleName) {
        return find(compoundTypeName);
    }

    @Override
    public NameEnvironmentAnswer findType(char[] name, char[][] packageName, char[] moduleName) {
        if (name == null) return null;
        return find(CharOperation.arrayConcat(packageName == null ? CharOperation.NO_CHAR_CHAR : packageName, name));
    }

    @Override
    public NameEnvironmentAnswer findType(char[][] compoundTypeName) {
        return find(compoundTypeName);
    }

    @Override
    public NameEnvironmentAnswer findType(char[] typeName, char[][] packageName) {
        return findType(typeName, packageName, null);
    }

    @Override
    public NameEnvironmentAnswer findTypeInModules(char[][] compoundTypeName, ModuleBinding module) {
        return find(compoundTypeName);
    }

    private boolean isPackageName(char[][] packageName) {
        if (packageName == null || packageName.length == 0) return true;
        String dotted = CharOperation.toString(packageName);
        return index.isPackage(dotted) || binaries.isPackage(
                CharOperation.subarray(packageName, 0, packageName.length - 1), packageName[packageName.length - 1]);
    }

    @Override
    public boolean isPackage(char[][] parentPackageName, char[] packageName) {
        char[][] full = CharOperation.arrayConcat(parentPackageName == null ? CharOperation.NO_CHAR_CHAR : parentPackageName,
                packageName);
        return isPackageName(full);
    }

    @Override
    public char[][] getModulesDeclaringPackage(char[][] packageName, char[] moduleName) {
        return isPackageName(packageName) ? new char[][] { ModuleBinding.UNNAMED } : null;
    }

    @Override
    public boolean hasCompilationUnit(char[][] pkgName, char[] moduleName, boolean checkCUs) {
        return isPackageName(pkgName);
    }

    @Override
    public IModule getModule(char[] name) {
        return null;
    }

    @Override
    public char[][] getAllAutomaticModules() {
        return CharOperation.NO_CHAR_CHAR;
    }

    @Override
    public void applyModuleUpdates(IUpdatableModule module, UpdateKind kind) {
        // classpath mode: no module updates
    }

    @Override
    public char[][] listPackages(char[] moduleName) {
        return CharOperation.NO_CHAR_CHAR;
    }

    @Override
    public boolean isOnModulePath(ICompilationUnit unit) {
        return false;
    }

    @Override
    public void cleanup() {
        // nothing per request: binary roots are shared caches
    }

    // ── Searches ─────────────────────────────────────────────────────────────

    @Override
    public void findModules(char[] prefix, ISearchRequestor requestor, IJavaProject javaProject) {
        // classpath mode: no modules
    }

    @Override
    public void findPackages(char[] prefix, ISearchRequestor requestor) {
        String p = new String(prefix);
        for (String pkg : sorted(index.allPackages())) {
            if (pkg.regionMatches(true, 0, p, 0, p.length())) {
                requestor.acceptPackage(pkg.toCharArray());
            }
        }
    }

    @Override
    public void findPackages(char[] prefix, ISearchRequestor requestor, IPackageFragmentRoot[] moduleContext,
            boolean followRequires) {
        findPackages(prefix, requestor);
    }

    private static List<String> sorted(Set<String> set) {
        List<String> list = new ArrayList<>(set);
        list.sort(null);
        return list;
    }

    private static boolean accepts(int searchFor, int modifiers) {
        boolean isInterface = (modifiers & ClassFileConstants.AccInterface) != 0;
        boolean isAnnotation = (modifiers & ClassFileConstants.AccAnnotation) != 0;
        boolean isEnum = (modifiers & ClassFileConstants.AccEnum) != 0;
        boolean isClass = !isInterface && !isEnum;
        return switch (searchFor) {
            case IJavaSearchConstants.CLASS -> isClass;
            case IJavaSearchConstants.INTERFACE -> isInterface;
            case IJavaSearchConstants.INTERFACE_AND_ANNOTATION -> isInterface;
            case IJavaSearchConstants.ENUM -> isEnum;
            case IJavaSearchConstants.ANNOTATION_TYPE -> isAnnotation;
            case IJavaSearchConstants.CLASS_AND_ENUM -> isClass || isEnum;
            case IJavaSearchConstants.CLASS_AND_INTERFACE -> isClass || isInterface;
            default -> true;
        };
    }

    @Override
    public void findTypes(char[] prefix, boolean findMembers, boolean camelCaseMatch, int searchFor, ISearchRequestor storage) {
        findTypes(prefix, findMembers, camelCaseMatch ? (TypeIndex.Matching.R_PREFIX_MATCH | TypeIndex.Matching.R_CAMELCASE_MATCH)
                : TypeIndex.Matching.R_PREFIX_MATCH, searchFor, true, storage, null);
    }

    @Override
    public void findTypes(char[] prefix, boolean findMembers, int matchRule, int searchFor, ISearchRequestor storage,
            IProgressMonitor monitor) {
        findTypes(prefix, findMembers, matchRule, searchFor, true, storage, monitor);
    }

    @Override
    public void findTypes(char[] prefix, boolean findMembers, int matchRule, int searchFor, boolean resolveDocumentName,
            ISearchRequestor storage, IProgressMonitor monitor) {
        int lastDot = CharOperation.lastIndexOf('.', prefix);
        char[] qualification = lastDot < 0 ? null : CharOperation.subarray(prefix, 0, lastDot);
        char[] simpleName = lastDot < 0 ? prefix : CharOperation.subarray(prefix, lastDot + 1, prefix.length);
        if ((matchRule & TypeIndex.Matching.R_CAMELCASE_MATCH) == 0) {
            simpleName = CharOperation.toLowerCase(simpleName);
        }
        Set<String> seen = new HashSet<>();
        index.searchTypes(qualification, simpleName, matchRule, findMembers, t -> {
            if (!accepts(searchFor, t.modifiers)) return;
            if (!seen.add(t.qualifiedName())) return;
            storage.acceptType(t.packageName.toCharArray(), t.simpleName.toCharArray(), enclosing(t), t.modifiers, null);
        });
    }

    @Override
    public void findExactTypes(char[] name, boolean findMembers, int searchFor, ISearchRequestor storage) {
        Set<String> seen = new HashSet<>();
        index.searchTypes(null, name, TypeIndex.Matching.R_EXACT_MATCH | TypeIndex.Matching.R_CASE_SENSITIVE, findMembers, t -> {
            if (!accepts(searchFor, t.modifiers)) return;
            if (!seen.add(t.qualifiedName())) return;
            storage.acceptType(t.packageName.toCharArray(), t.simpleName.toCharArray(), enclosing(t), t.modifiers, null);
        });
    }

    private static char[][] enclosing(TypeIndex.TypeInfo t) {
        char[][] e = new char[t.enclosingNames.length][];
        for (int i = 0; i < e.length; i++) e[i] = t.enclosingNames[i].toCharArray();
        return e;
    }

    @Override
    public void findConstructorDeclarations(char[] prefix, int matchRule, boolean resolveDocumentName, ISearchRequestor storage,
            IProgressMonitor monitor) {
        int lastDot = CharOperation.lastIndexOf('.', prefix);
        char[] qualification = lastDot < 0 ? null : CharOperation.subarray(prefix, 0, lastDot);
        char[] simpleName = lastDot < 0 ? prefix : CharOperation.subarray(prefix, lastDot + 1, prefix.length);
        if ((matchRule & TypeIndex.Matching.R_CAMELCASE_MATCH) == 0) {
            simpleName = CharOperation.toLowerCase(simpleName);
        }
        Set<String> seen = new HashSet<>();
        index.searchTypes(qualification, simpleName, matchRule, false, t -> {
            if (!seen.add(t.qualifiedName())) return;
            if ((t.modifiers & (ClassFileConstants.AccInterface | ClassFileConstants.AccEnum | ClassFileConstants.AccAnnotation)) != 0
                    && (t.modifiers & ClassFileConstants.AccInterface) != 0) {
                // interfaces have no constructors; anonymous proposals come from the type itself
                return;
            }
            int extraFlags = 0;
            char[] pkg = t.packageName.toCharArray();
            char[] simple = t.simpleName.toCharArray();
            if (t.constructors.isEmpty()) {
                // default constructor
                int mods = t.modifiers & (ClassFileConstants.AccPublic | ClassFileConstants.AccProtected | ClassFileConstants.AccPrivate);
                if ((t.modifiers & ExtraCompilerModifiers.AccRecord) == 0 || !t.isSource) {
                    storage.acceptConstructor(t.isSource ? mods : ClassFileConstants.AccPublic & t.modifiers, simple, 0,
                            null, CharOperation.NO_CHAR_CHAR, CharOperation.NO_CHAR_CHAR, t.modifiers, pkg,
                            extraFlags, t.path, null);
                } else {
                    storage.acceptConstructor(mods, simple, -1, null, null, null, t.modifiers, pkg, extraFlags, t.path, null);
                }
                return;
            }
            for (TypeIndex.CtorInfo c : t.constructors) {
                int count = c.signature != null ? org.eclipse.jdt.core.Signature.getParameterCount(c.signature)
                        : c.parameterTypes.length;
                storage.acceptConstructor(c.modifiers, simple, count, c.signature, c.parameterTypes, c.parameterNames,
                        t.modifiers, pkg, extraFlags, t.path, null);
            }
        });
    }
}
