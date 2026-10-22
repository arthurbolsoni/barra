fn main() {
    // seção com o modo pedido pela barra, a mesma memória em todos os processos que carregam a DLL
    println!("cargo:rustc-cdylib-link-arg=/SECTION:.barra,RWS");
}
