fn main() {
    let engine_dir = "../../tools/vocal-engine";
    let world_src = "../../tools/vocal-engine/vendor/world/src";
    let snaptune_dir = "../../crates/os-dsp/vendor/snaptune";
    println!("cargo:rerun-if-changed={engine_dir}/bridge.cpp");
    println!("cargo:rerun-if-changed={world_src}");
    println!("cargo:rerun-if-changed={snaptune_dir}");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include(world_src)
        .include(snaptune_dir)
        .file(format!("{engine_dir}/bridge.cpp"))
        .file(format!("{world_src}/cheaptrick.cpp"))
        .file(format!("{world_src}/codec.cpp"))
        .file(format!("{world_src}/common.cpp"))
        .file(format!("{world_src}/d4c.cpp"))
        .file(format!("{world_src}/dio.cpp"))
        .file(format!("{world_src}/fft.cpp"))
        .file(format!("{world_src}/harvest.cpp"))
        .file(format!("{world_src}/matlabfunctions.cpp"))
        .file(format!("{world_src}/stonemask.cpp"))
        .file(format!("{world_src}/synthesis.cpp"))
        .file(format!("{world_src}/synthesisrealtime.cpp"))
        .file(format!("{snaptune_dir}/AutoTuneEngine.cpp"))
        .file(format!("{snaptune_dir}/PitchDetector.cpp"))
        .file(format!("{snaptune_dir}/bridge.cpp"))
        .flag_if_supported("/EHsc")
        .flag_if_supported("-Wno-unused-parameter")
        .compile("lemon_vocal_world");
}
