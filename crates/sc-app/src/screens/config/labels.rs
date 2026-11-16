//! Plain names and tidied descriptions for the cvars in the catalogue.

use sc_core::catalog::{attrs, cvars::Cvar};
use std::collections::HashMap;
use std::sync::OnceLock;

const LABELS: [(&str, &str); 22] = [
    (
        "g_headtracking_hmd_fpsMovementMode",
        "Headset movement on foot",
    ),
    (
        "g_headtracking_hmd_fpsTriggerResetOnZoom",
        "Reset headset view on aim",
    ),
    (
        "g_headtracking_hmd_lensDisplayEnabled",
        "Headset lens display",
    ),
    (
        "g_headtracking_hmd_fpsAdsDominantEye",
        "Aim with the dominant eye",
    ),
    ("g_headtracking_hmd_cursorSize", "Headset cursor size"),
    (
        "g_headtracking_hmd_cursorOffsetSensitivity",
        "Headset cursor sensitivity",
    ),
    (
        "g_headtracking_hmd_cursorFollowsHead",
        "Headset cursor follows head",
    ),
    (
        "g_headtracking_hmd_cursorUseEyeTracking",
        "Headset cursor eye tracking",
    ),
    ("g_language", "Language"),
    ("Con_Restricted", "Restricted console"),
    ("r.graphicsRenderer", "Vulkan renderer"),
    ("cl_fov", "Field of view"),
    ("cl_fovCombat", "Field of view in combat"),
    ("cl_fovSocial", "Field of view in social areas"),
    ("r_DisplayInfo", "Performance overlay"),
    ("r_DisplaySessionInfo", "Session QR code"),
    ("r_DisplayFrameGraph", "Frame time graph"),
    ("r_DepthOfField", "Depth of field"),
    ("r_MotionBlur", "Motion blur"),
    ("r_filmgrain", "Film grain"),
    ("r_vignetteBlur", "Vignette blur"),
    ("r_ssdo", "Ambient occlusion"),
];

/// The row label for a cvar: the head tracking attribute's label when it has one, then the
/// table above, then the cvar name itself.
pub fn label(cvar: &'static Cvar) -> &'static str {
    if let Some(known) = cvar.attribute.as_deref().and_then(attrs::find) {
        return &known.label;
    }
    LABELS
        .iter()
        .find(|(name, _)| *name == cvar.name)
        .map_or(cvar.name.as_str(), |(_, label)| label)
}

/// The catalogue description with a capital first letter and without wrapping brackets.
pub fn description(cvar: &'static Cvar) -> Option<&'static str> {
    static TIDY: OnceLock<HashMap<String, String>> = OnceLock::new();
    let tidy = TIDY.get_or_init(|| {
        sc_core::catalog::cvars::all()
            .iter()
            .filter(|cvar| !cvar.description.trim().is_empty())
            .map(|cvar| (cvar.name.clone(), tidied(&cvar.description)))
            .collect()
    });
    tidy.get(&cvar.name).map(String::as_str)
}

fn tidied(text: &str) -> String {
    let text = text.trim();
    let text = text
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(text)
        .trim();
    let text = text.replace(".)", ")");
    let mut chars = text.chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    out.push_str(chars.as_str());
    if !out.ends_with(['.', ')']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::catalog::cvars;

    #[test]
    fn tidied_capitalises_and_drops_wrapping_brackets() {
        assert_eq!(tidied("(0 disabled, 1 enabled.)"), "0 disabled, 1 enabled.");
        assert_eq!(
            tidied("show the QR Code (0 disabled, 1 enabled.)"),
            "Show the QR Code (0 disabled, 1 enabled)"
        );
        assert_eq!(
            tidied("0 disabled, 1 vehicle blur"),
            "0 disabled, 1 vehicle blur."
        );
    }

    #[test]
    fn every_cvar_gets_a_plain_label() {
        for cvar in cvars::all() {
            let label = label(cvar);
            assert!(!label.is_empty());
            assert!(
                !label.contains('_') && !label.contains('.'),
                "{} has no plain label",
                cvar.name
            );
        }
        assert_eq!(label(cvars::find("cl_fov").unwrap()), "Field of view");
        assert_eq!(
            label(cvars::find("g_headtrackingToggleEnabled").unwrap()),
            "Enabled"
        );
    }

    #[test]
    fn descriptions_are_tidied_once() {
        let cvar = cvars::find("r_filmgrain").unwrap();
        assert_eq!(description(cvar), Some("0 disabled, 1 enabled."));
        assert_eq!(description(cvars::find("cl_fov").unwrap()), None);
    }
}
