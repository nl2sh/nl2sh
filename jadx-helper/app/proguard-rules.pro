-keep class com.nl2sh.jadx.Main {
    public static void main(java.lang.String[]);
}

-keep class jadx.plugins.input.dex.DexInputPlugin { *; }
-keepattributes RuntimeVisibleAnnotations,RuntimeInvisibleAnnotations,AnnotationDefault,Signature,InnerClasses,EnclosingMethod
-dontwarn org.jetbrains.annotations.**
