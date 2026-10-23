fn main() {
    // seção com o caminho da barra_menus.dll, a mesma memória em todos os processos que carregam a DLL
    println!("cargo:rustc-cdylib-link-arg=/SECTION:.barra,RWS");
    // sem data nem identificador aleatório no binário: mesmo código, mesmos bytes, mesma cópia
    println!("cargo:rustc-cdylib-link-arg=/Brepro");
}
