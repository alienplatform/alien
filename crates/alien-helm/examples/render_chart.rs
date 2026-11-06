//! Write a generated chart to a directory, for `helm lint`, `helm install` and kind runs.
//!
//! Usage: `render_chart <out-dir> [<fingerprint> <channel> <operator-digest>]`
//!
//! Without signing arguments the chart is unprotected.

use alien_core::{Stack, StackSettings, StackSigning};
use alien_helm::{
    generate_helm_chart, generate_signed_helm_chart, HelmOptions, HelmRegistry, SignedChartPins,
};
use std::{env, fs, path::Path};

fn main() {
    let args: Vec<String> = env::args().collect();
    let out = Path::new(args.get(1).expect("usage: render_chart <out-dir> [...]"));
    let stack = Stack::new("signing-test".to_string()).build();
    let registry = HelmRegistry::built_in();
    let options = |stack_settings| HelmOptions {
        registry: &registry,
        stack_settings,
        chart_name: "signing-test".to_string(),
    };
    let chart = match args.as_slice() {
        [_, _] => generate_helm_chart(&stack, options(StackSettings::default())),
        [_, _, fingerprint, channel, digest] => {
            let settings = StackSettings {
                signing: Some(StackSigning {
                    private_input_custody: false,
                    private_machine_api_url: None,
                    fingerprint: fingerprint.clone(),
                    channel: channel.clone(),
                }),
                ..StackSettings::default()
            };
            let pins = SignedChartPins {
                operator_digest: digest.clone(),
            };
            generate_signed_helm_chart(&stack, options(settings), &pins)
        }
        _ => panic!("usage: render_chart <out-dir> [<fingerprint> <channel> <operator-digest>]"),
    }
    .unwrap_or_else(|error| panic!("render failed: {error}"));
    for (path, contents) in chart.files {
        let destination = out.join(path);
        fs::create_dir_all(destination.parent().expect("parent")).expect("mkdir");
        fs::write(destination, contents).expect("write");
    }
}
