//! Native-messaging host started by Chrome or Edge for the BDM extension.
//! See `bdm_lib::browser::host`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(env) = bdm_lib::browser::host::HostEnv::from_system() else {
        std::process::exit(1);
    };
    let code = bdm_lib::browser::host::run(&args, &mut std::io::stdin().lock(), &mut std::io::stdout().lock(), &env);
    std::process::exit(code);
}
