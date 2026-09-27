package com.nl2sh.jadx;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

import jadx.api.JadxArgs;
import jadx.api.JadxDecompiler;
import jadx.api.JavaClass;

/** Small ART entrypoint for one-class JADX decompilation. */
public final class Main {
    private Main() {
    }

    public static void main(String[] args) {
        if (args.length != 3) {
            System.err.println("usage: Main <apk> <class-name> <output-java>");
            System.exit(2);
            return;
        }

        JadxArgs jadxArgs = new JadxArgs();
        jadxArgs.setInputFile(new File(args[0]));
        jadxArgs.setSkipResources(true);
        jadxArgs.setThreadsCount(2);
        try (JadxDecompiler jadx = new JadxDecompiler(jadxArgs)) {
            jadx.load();
            for (JavaClass cls : jadx.getClassesWithInners()) {
                if (args[1].equals(cls.getRawName()) || args[1].equals(cls.getFullName())) {
                    String code = cls.getCode();
                    if (code == null || code.isEmpty()) {
                        System.err.println("JADX returned empty source for " + args[1]);
                        System.exit(3);
                        return;
                    }
                    File output = new File(args[2]);
                    try (FileOutputStream stream = new FileOutputStream(output)) {
                        stream.write(code.getBytes(StandardCharsets.UTF_8));
                    }
                    return;
                }
            }
            System.err.println("Class not found: " + args[1]);
            System.exit(4);
        } catch (Exception error) {
            error.printStackTrace(System.err);
            System.exit(5);
        }
    }
}
