Run `cargo xtask android-prepare` once, then open this directory in Android Studio.

Ruwriter stores its library under `COVE_APP_STORAGE_DIR` (the Activity's
`filesDir`) with `PACKAGE_RESOURCE_BUNDLE_PATH` and `HOME` as fallbacks. Opening
files uses the in-app picker provided by the Cove host.

Each Gradle build runs `cargo xtask android-prepare` to build the Rust library
and stage assets.
