// Rebuild when a migration is added or changed: `sqlx::migrate!` builds them into the binary, and
// cargo does not see a change in that folder by itself (the build script `sqlx migrate build-script` writes).
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
