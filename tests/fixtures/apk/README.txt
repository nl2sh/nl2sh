Static analyzer fixtures contain no executable Android application.

Probe.java is compiled with JDK 17 javac --release 8, then Android SDK build-tools
35.0.0 d8 --min-api 26 --lib <SDK>/platforms/android-35/android.jar.
The three generated example/*.class files produce classes.dex.

AndroidManifest.axml is extracted from the output of SDK 35.0.0 aapt2 link
--manifest AndroidManifest.xml -I <SDK>/platforms/android-35/android.jar -o fixture.apk.
The missing exported attribute on NeedsReview intentionally represents an
invalid modern component requiring review; it is never installed.
