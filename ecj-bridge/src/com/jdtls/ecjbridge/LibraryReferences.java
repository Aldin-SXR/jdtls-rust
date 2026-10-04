package com.jdtls.ecjbridge;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

import org.eclipse.jdt.core.dom.IBinding;
import org.eclipse.jdt.core.dom.IMethodBinding;
import org.eclipse.jdt.core.dom.ITypeBinding;
import org.eclipse.jdt.core.dom.IVariableBinding;
import org.eclipse.jdt.core.util.IClassFileReader;
import org.eclipse.jdt.core.util.IConstantPool;
import org.eclipse.jdt.core.util.IConstantPoolConstant;
import org.eclipse.jdt.core.util.IConstantPoolEntry;

import com.jdtls.ecjbridge.ClassFileService.ClassFileDesc;

/**
 * Library class files that reference a binary element — the binary part of
 * the JDT search index, computed from constant pools on demand.
 */
final class LibraryReferences {

    private LibraryReferences() {}

    static List<ClassFileDesc> candidates(String library, IBinding element, ITypeBinding owner) {
        List<ClassFileDesc> out = new ArrayList<>();
        File f = new File(library);
        if (!f.isFile()) {
            return out;
        }
        String ownerInternal = owner.getTypeDeclaration().getBinaryName();
        if (ownerInternal == null) {
            return out;
        }
        ownerInternal = ownerInternal.replace('.', '/');
        String member = element instanceof ITypeBinding ? null : element.getName();
        byte[] needle = ownerInternal.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        List<String> entries = new ArrayList<>(ClassFileService.jarEntries(library));
        java.util.Collections.sort(entries);
        for (String entry : entries) {
            if (!entry.endsWith(".class") || entry.endsWith("module-info.class")) {
                continue;
            }
            int slash = entry.lastIndexOf('/');
            ClassFileDesc d = new ClassFileDesc();
            d.root = library;
            d.packageName = slash < 0 ? "" : entry.substring(0, slash).replace('/', '.');
            d.classFileName = entry.substring(slash + 1);
            byte[] bytes = ClassFileService.bytes(d);
            if (bytes == null || !contains(bytes, needle)) {
                continue;
            }
            String self = entry.substring(0, entry.length() - ".class".length());
            if (references(bytes, self, ownerInternal, member, element)) {
                out.add(d);
            }
        }
        return out;
    }

    /** Class files of the JDK (jrt modules in name order) referencing {@code element}. */
    static List<ClassFileDesc> jrtCandidates(IBinding element, ITypeBinding owner) {
        List<ClassFileDesc> out = new ArrayList<>();
        java.nio.file.FileSystem fs = ClassFileService.jrt();
        String ownerInternal = owner.getTypeDeclaration().getBinaryName();
        if (fs == null || ownerInternal == null) {
            return out;
        }
        ownerInternal = ownerInternal.replace('.', '/');
        String member = element instanceof ITypeBinding ? null : element.getName();
        byte[] needle = ownerInternal.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        List<String> modules = new ArrayList<>();
        try (java.util.stream.Stream<java.nio.file.Path> s = java.nio.file.Files.list(fs.getPath("/modules"))) {
            s.forEach(p -> modules.add(p.getFileName().toString()));
        } catch (java.io.IOException e) {
            return out;
        }
        java.util.Collections.sort(modules);
        String root = ClassFileService.jrtRoot();
        for (String module : modules) {
            java.nio.file.Path base = fs.getPath("/modules", module);
            List<String> entries = new ArrayList<>();
            try (java.util.stream.Stream<java.nio.file.Path> s = java.nio.file.Files.walk(base)) {
                s.forEach(p -> {
                    String rel = base.relativize(p).toString();
                    if (rel.endsWith(".class") && !rel.endsWith("module-info.class")) {
                        entries.add(rel);
                    }
                });
            } catch (java.io.IOException e) {
                continue;
            }
            java.util.Collections.sort(entries);
            for (String entry : entries) {
                byte[] bytes;
                try {
                    bytes = java.nio.file.Files.readAllBytes(base.resolve(entry));
                } catch (java.io.IOException e) {
                    continue;
                }
                if (!contains(bytes, needle)) {
                    continue;
                }
                String self = entry.substring(0, entry.length() - ".class".length());
                if (references(bytes, self, ownerInternal, member, element)) {
                    int slash = entry.lastIndexOf('/');
                    ClassFileDesc d = new ClassFileDesc();
                    d.root = root;
                    d.module = module;
                    d.packageName = slash < 0 ? "" : entry.substring(0, slash).replace('/', '.');
                    d.classFileName = entry.substring(slash + 1);
                    out.add(d);
                }
            }
        }
        return out;
    }

    private static boolean references(byte[] bytes, String self, String owner, String member, IBinding element) {
        IClassFileReader reader;
        try {
            reader = new org.eclipse.jdt.internal.core.util.ClassFileReader(bytes, IClassFileReader.CONSTANT_POOL);
        } catch (Exception e) {
            return false;
        }
        IConstantPool pool = reader.getConstantPool();
        String descriptor = "L" + owner + ";";
        for (int i = 1; i < pool.getConstantPoolCount(); i++) {
            int kind = pool.getEntryKind(i);
            IConstantPoolEntry e;
            switch (kind) {
                case IConstantPoolConstant.CONSTANT_Class -> {
                    if (member == null && !self.equals(owner) && !self.startsWith(owner + "$")) {
                        e = pool.decodeEntry(i);
                        if (owner.equals(new String(e.getClassInfoName()))) {
                            return true;
                        }
                    }
                }
                case IConstantPoolConstant.CONSTANT_Utf8 -> {
                    if (member == null && !self.equals(owner) && !self.startsWith(owner + "$")) {
                        e = pool.decodeEntry(i);
                        if (new String(e.getUtf8Value()).contains(descriptor)) {
                            return true;
                        }
                    }
                }
                case IConstantPoolConstant.CONSTANT_Fieldref -> {
                    if (element instanceof IVariableBinding) {
                        e = pool.decodeEntry(i);
                        if (owner.equals(new String(e.getClassName())) && member.equals(new String(e.getFieldName()))) {
                            return true;
                        }
                    }
                }
                case IConstantPoolConstant.CONSTANT_Methodref, IConstantPoolConstant.CONSTANT_InterfaceMethodref -> {
                    if (element instanceof IMethodBinding m) {
                        e = pool.decodeEntry(i);
                        String name = m.isConstructor() ? "<init>" : member;
                        if (owner.equals(new String(e.getClassName())) && name.equals(new String(e.getMethodName()))) {
                            return true;
                        }
                    }
                }
                default -> {
                }
            }
        }
        return false;
    }

    private static boolean contains(byte[] hay, byte[] needle) {
        outer:
        for (int i = 0; i + needle.length <= hay.length; i++) {
            for (int j = 0; j < needle.length; j++) {
                if (hay[i + j] != needle[j]) {
                    continue outer;
                }
            }
            return true;
        }
        return false;
    }
}
