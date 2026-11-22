//! Effect members only Studio has (Bars' Step on Marks, #67; Spiral's, B51).
//!
//! Real Vixen and the vanilla port ignore or keep members they don't know, but writing them on every effect would
//! change files nobody edited that way. So a Studio member is left out while it holds its table default, and a file
//! without it (any file from Windows Vixen or vanilla) reads as that default rather than C#'s `default(T)`.

use vixen_effects::framework::{ParamDesc, ParamValue};

/// (CLR namespace of the effect's data class, member name).
const MEMBERS: [(&str, &str); 6] = [
    ("VixenModules.Effect.Bars", "MarkCollectionId"),
    ("VixenModules.Effect.Bars", "StepsPerCycle"),
    ("VixenModules.Effect.Bars", "StepToMarks"),
    ("VixenModules.Effect.Spiral", "MarkCollectionId"),
    ("VixenModules.Effect.Spiral", "StepsPerCycle"),
    ("VixenModules.Effect.Spiral", "StepToMarks"),
];

pub(crate) fn is_studio_member(d: &ParamDesc) -> bool {
    MEMBERS.iter().any(|(n, m)| *n == d.xml_ns && *m == d.xml)
}

/// Whether to leave this member out of the file: a Studio member at its default.
pub(crate) fn skipped(d: &ParamDesc, v: &ParamValue) -> bool {
    is_studio_member(d) && *v == (d.default)()
}

#[cfg(test)]
mod tests {
    use crate::params::{effect_model_to_xml, effect_params_from_xml, params_to_members};
    use vixen_effects::framework::{EffectDescriptor, ParamValue, Params};

    fn effect(name: &str) -> &'static EffectDescriptor {
        vixen_effect_registry::all().iter().copied().find(|d| d.name == name).expect(name)
    }

    /// The effects with Step on Marks, and their default steps per cycle.
    const STEPPING: [(&str, i32); 2] = [("Bars", 4), ("Spiral", 3)];

    fn round_trip(d: &'static EffectDescriptor, p: &Params) -> Params {
        let model = effect_model_to_xml(&d.type_id, &uuid::Uuid::new_v4(), p).unwrap();
        effect_params_from_xml(&model, d)
    }

    #[test]
    fn untouched_effects_write_no_studio_members_and_read_back_their_defaults() {
        for (name, per_cycle) in STEPPING {
            let d = effect(name);
            let members = params_to_members(&Params::defaults(d.params));
            assert!(!members.iter().any(|m| matches!(m.local.as_str(), "StepToMarks" | "MarkCollectionId" | "StepsPerCycle")), "{name}");
            let back = round_trip(d, &Params::defaults(d.params));
            assert!(!back.boolean("StepToMarks"), "{name}");
            assert_eq!(back.int("StepsPerCycle"), per_cycle, "{name}");
        }
    }

    #[test]
    fn edited_studio_members_round_trip() {
        for (name, _) in STEPPING {
            let d = effect(name);
            let mut p = Params::defaults(d.params);
            let id = uuid::Uuid::new_v4();
            p.set("StepToMarks", ParamValue::Bool(true));
            p.set("MarkCollectionId", ParamValue::Guid(id));
            p.set("StepsPerCycle", ParamValue::Int(8));
            let back = round_trip(d, &p);
            assert!(back.boolean("StepToMarks"), "{name}");
            assert_eq!(back.guid("MarkCollectionId"), id, "{name}");
            assert_eq!(back.int("StepsPerCycle"), 8, "{name}");
        }
    }
}
