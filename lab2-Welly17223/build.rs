fn main() {
    println!("cargo:rustc-link-arg-bin=os=-Tlink_script.ld");
}
