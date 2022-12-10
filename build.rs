extern crate bindgen;
use regex::Regex;
use std::env;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Write, };
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rustc-link-search=/usr/lib");
    println!("cargo:rustc-link-lib=dylib=brp_lib");
    println!("cargo:rustc-link-lib=static=baltech_api");

    let bindings = bindgen::Builder::default()
        .layout_tests(false)
        //.emit_ir()
        .header("include/brp_lib/names.h")
        .header("include/brp_lib.h")
        .header("include/baltech_api.h")
        .allowlist_function("(brp_open|brp_close|brp_flush|brp_destroy|brp_map_errcode|brp_map_errcode_to_desc|brp_mempool_free)")
        .allowlist_function("(brp_create_rs232|brp_Sys_GetInfo|brp_set_io|brp_create)")
        .allowlist_function("(brp_Desfire_.*|brp_VHL_.*)")
        .allowlist_var("BRP_.*")
        //.blocklist_item("IPPORT.*")
        //.parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/api.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("api.rs"))
        .expect("Couldn't write bindings!");

    let file_path = out_path.join("brp_wrapper.rs");
    let file = File::create(&file_path).unwrap();
    let re = Regex::new(r#"pub fn (\w+)\(([^\)]+)\) -> brp_errcode"#).unwrap();
    let rere = Regex::new(r#"(\w+): (.+)"#).unwrap();
    let rere_norm = Regex::new(r#","#).unwrap();
    //let rere_clean = Regex::new(r#"[\n\s]+"#).unwrap();
    let rere_split = Regex::new(r#"\s+"#).unwrap();
    let mut mut_par_tag: &str;
    for cap in re.captures_iter(bindings.to_string().as_str()) {
        let name = &cap[1];
        //let par = &*rere_norm.replace_all(&cap[2], ",\n");
        //for par_pair in par.split(',').i
        let mut call_pars = String::default();
        let mut struct_types = String::default();
        let mut needs_mut_par = false;
        rere_norm.split(&cap[2]).for_each(|par_norm|{
            let new_par = par_norm.replace("\n", "").trim().to_string();
            if new_par.is_empty() {
                return;
            }

            for cap_arg in rere.captures_iter(&new_par) {
                let par_name = &cap_arg[1];
                let parts = rere_split.split(&cap_arg[2]);
                let par_type = &parts.last().unwrap();
                let cnt = rere_split.split(&cap_arg[2]);
                let par_ref_count = cnt.count() - 1;
                /*if ["brp_buff", "brp_mempool", "bool"].contains( &par_type) { // Special handling if needed
                    ...
                } else ...*/
                if par_ref_count == 0_usize {
                    let _ = write!(struct_types, "{}: Buf::<{}>,\n", par_name, par_type);
                    let _ = write!(call_pars, "p.{}.0.assume_init()\n", par_name);
                } else if par_ref_count == 1_usize {
                    let _ = write!(struct_types, "{}: Buf::<{}>,\n", par_name, par_type);
                    needs_mut_par = true;
                    dbg!(&new_par); //dbg!(par_ref_count); dbg!(par_type); dbg!(par_name);
                    let _ = write!(call_pars, "p.{}.0.as_mut_ptr()\n", par_name);
                } else if par_ref_count == 2_usize {
                    let _ = write!(struct_types, "{}: Buf::<*mut {}>,\n", par_name, par_type);
                    let _ = write!(call_pars, "p.{}.0.as_mut_ptr() as *mut *mut {}\n", par_name, par_type);
                } else {
                    todo!();
                }
                let _ = write!(call_pars, ",\n");
            }
        });
        if needs_mut_par {
            mut_par_tag = "mut";
        } else {
            mut_par_tag = "";
        }

        let _ = writeln!(
            &file,
r#"#[derive(Debug)]
pub struct par_{name} {{{struct_types}}}
#[logfn(Debug)]
#[logfn_inputs(Trace)]
pub fn gen_{name}({mut_par_tag} p: par_{name}) -> BrpResult<par_{name}>
{{ unsafe {{
    {name}({call_pars})
}}.try_ok(p) }}"#);
    }
    dbg!(&file_path);
    let _ = Command::new("rustfmt").arg(file_path.display().to_string()).spawn();
}
