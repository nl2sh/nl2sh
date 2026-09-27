package com.nl2sh.jadx;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Collections;
import java.util.EnumSet;

import org.w3c.dom.Document;

import jadx.api.DecompilationMode;
import jadx.api.JadxArgs;
import jadx.api.JadxDecompiler;
import jadx.api.JavaClass;
import jadx.api.impl.NoOpCodeCache;
import jadx.api.security.IJadxSecurity;
import jadx.api.security.JadxSecurityFlag;
import jadx.api.security.impl.JadxSecurity;

/** Small ART entrypoint for one-class JADX decompilation. */
public final class Main {
    private static final IJadxSecurity CLASS_ONLY_SECURITY = new ClassOnlySecurity();

    private Main() {
    }

    private static final class ClassOnlySecurity implements IJadxSecurity {
        private final JadxSecurity packageVerifier =
                new JadxSecurity(EnumSet.of(JadxSecurityFlag.VERIFY_APP_PACKAGE));

        @Override
        public String verifyAppPackage(String appPackage) {
            return packageVerifier.verifyAppPackage(appPackage);
        }

        @Override
        public Document parseXml(InputStream input) {
            throw new SecurityException("XML parsing is disabled in the class-only helper");
        }
    }

    public static void main(String[] args) {
        if (args.length != 3) {
            System.err.println("usage: Main <apk> <class-name> <output-java>");
            System.exit(2);
            return;
        }

        String requestedClass = args[1];
        JadxArgs jadxArgs = new JadxArgs();
        jadxArgs.setInputFile(new File(args[0]));
        jadxArgs.setSkipResources(true);
        jadxArgs.setThreadsCount(1);
        jadxArgs.setCodeCache(new NoOpCodeCache());
        jadxArgs.setSecurity(CLASS_ONLY_SECURITY);
        jadxArgs.setDecompilationMode(DecompilationMode.SIMPLE);
        jadxArgs.setUseImports(false);
        jadxArgs.setDebugInfo(false);
        jadxArgs.setInlineAnonymousClasses(false);
        jadxArgs.setInlineMethods(false);
        jadxArgs.setMoveInnerClasses(false);
        jadxArgs.setRenameFlags(Collections.emptySet());
        jadxArgs.setClassFilter(name -> requestedClass.equals(name)
                || name.startsWith(requestedClass + "$"));
        try (JadxDecompiler jadx = new JadxDecompiler(jadxArgs)) {
            jadx.load();
            JavaClass cls = jadx.searchJavaClassByOrigFullName(requestedClass);
            if (cls == null) {
                System.err.println("Class not found: " + requestedClass);
                System.exit(4);
                return;
            }
            String code = cls.getCode();
            if (code == null || code.isEmpty()) {
                System.err.println("JADX returned empty source for " + requestedClass);
                System.exit(3);
                return;
            }
            File output = new File(args[2]);
            try (FileOutputStream stream = new FileOutputStream(output)) {
                stream.write(code.getBytes(StandardCharsets.UTF_8));
            }
        } catch (Exception error) {
            error.printStackTrace(System.err);
            System.exit(5);
        }
    }
}
