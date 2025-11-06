fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(false) // 不生成服务端代码
        .compile_protos(
            &["proto/agent.proto"],
            &["proto"], // proto 根目录
        )?;

    println!("cargo:rerun-if-changed=proto/agent.proto");
    Ok(())
}

