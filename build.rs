fn main() {
    println!("cargo:rerun-if-env-changed=VAPOR_PLATFORM_REVISION");
}
