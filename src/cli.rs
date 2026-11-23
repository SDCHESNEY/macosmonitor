use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "rtop",
    version,
    about = "A macOS-first terminal system monitor"
)]
pub struct Args {
    /// Time between system samples, in milliseconds (minimum 250).
    #[arg(
        long,
        default_value_t = 1000,
        value_parser = clap::value_parser!(u64).range(250..)
    )]
    pub refresh_ms: u64,

    #[cfg(feature = "test-panic")]
    #[arg(long, hide = true)]
    pub test_panic: bool,
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::Parser;

    #[test]
    fn refresh_interval_defaults_to_one_second() {
        let args = Args::try_parse_from(["rtop"]).expect("default CLI arguments should parse");

        assert_eq!(args.refresh_ms, 1000);
    }

    #[test]
    fn refresh_interval_rejects_values_below_250_ms() {
        assert!(Args::try_parse_from(["rtop", "--refresh-ms", "249"]).is_err());
        assert_eq!(
            Args::try_parse_from(["rtop", "--refresh-ms", "500"])
                .expect("500 ms should parse")
                .refresh_ms,
            500
        );
    }
}
