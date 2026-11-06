pub const CURVES: [(&'static str, &'static str); 5] = [
    ("auto", "hable"),
    ("clip", "clip"),
    ("mobius", "mobius"),
    ("reinhard", "reinhard"),
    ("hable", "hable"),
];

pub fn normalize_curve(key: &str) -> &'static str {
    CURVES
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(candidate, _)| *candidate)
        .unwrap_or("auto")
}

pub fn label(key: &str) -> &'static str {
    match normalize_curve(key) {
        "clip" => "Clip",
        "mobius" => "Mobius",
        "reinhard" => "Reinhard",
        "hable" => "Hable",
        _ => "自动（Hable）",
    }
}

pub fn is_hdr(gamma: &str) -> bool {
    gamma.eq_ignore_ascii_case("pq") || gamma.eq_ignore_ascii_case("hlg")
}

pub fn filter_chain(curve: &str, target_peak: u32, use_metadata: bool) -> String {
    let key = normalize_curve(curve);
    let tonemap = CURVES
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, name)| *name)
        .unwrap_or("hable");
    let npl = if target_peak == 0 { 100 } else { target_peak };
    let peak = if use_metadata { String::new() } else { ":peak=1000".to_string() };
    format!(
        "zscale=t=linear:npl={npl},format=gbrpf32le,zscale=p=bt709,tonemap=tonemap={tonemap}:desat=0{peak},zscale=t=bt709:m=bt709:r=tv,format=rgb24"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_resolves_every_curve() {
        for (key, tonemap) in CURVES {
            assert_eq!(normalize_curve(key), key);
            assert!(
                filter_chain(key, 0, true).contains(&format!("tonemap=tonemap={tonemap}:desat=0"))
            );
        }
    }

    #[test]
    fn unknown_curve_falls_back_to_auto() {
        assert_eq!(normalize_curve(""), "auto");
        assert_eq!(normalize_curve("bogus"), "auto");
        assert!(filter_chain("bogus", 0, true).contains("tonemap=tonemap=hable:desat=0"));
    }

    #[test]
    fn detects_hdr_gamma() {
        assert!(is_hdr("pq"));
        assert!(is_hdr("HLG"));
        assert!(!is_hdr("bt.1886"));
        assert!(!is_hdr(""));
    }

    #[test]
    fn uses_target_peak_for_npl() {
        assert!(filter_chain("hable", 0, true).contains("npl=100"));
        assert!(filter_chain("hable", 250, true).contains("npl=250"));
    }

    #[test]
    fn peak_override_only_without_metadata() {
        assert!(!filter_chain("hable", 300, true).contains("peak="));
        assert!(filter_chain("hable", 300, false).contains(":peak=1000"));
    }
}
