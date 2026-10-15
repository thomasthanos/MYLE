# Found by name from Rust (register_android_plugin): never renamed or removed.
-keep class uk.thomast.myle.mobile.MyleMobilePlugin { *; }

# Exported Rust JNI symbol: Java_uk_thomast_myle_mobile_NativeVault_call.
# Preserve both the object class and native method name if the app enables R8.
-keep class uk.thomast.myle.mobile.NativeVault { *; }
