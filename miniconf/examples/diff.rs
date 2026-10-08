//! Stream a leaf diff, collect a reversible change set, then apply or undo it.
//! Diff reports unequal JSON values and read failures on either side. Its consumer
//! chooses whether to continue; a patch requires successful reads on both sides.
//! Collect a complete patch before applying it.
//! Apply stops on error. Earlier writes remain applied; even the failing setter
//! may have side effects. Undo assumes round-trippable leaves, a target initially
//! matching `before`, and no intervening edits; it cannot reverse hardware effects.

use core::{ops::ControlFlow, str};

use miniconf::{
    Indices, IntoKeys, Path, SerdeError, TreeDeserializeOwned, TreeSerialize, ValueError, json_core,
};
use serde_json_core::{de, ser};

mod common;
use common::Settings;

type Read<'a> = Result<&'a [u8], SerdeError<ser::Error>>;

// Allocation-free traversal and payloads. The callback borrows each change;
// callers choose whether to inspect or retain it.
fn diff<T: TreeSerialize, B, const D: usize>(
    before: &T,
    after: &T,
    buffers: [&mut [u8]; 2],
    mut emit: impl FnMut(Indices<[usize; D]>, Read<'_>, Read<'_>) -> ControlFlow<B>,
) -> ControlFlow<B>
where
    [usize; D]: Default,
{
    let [left, right] = buffers;
    for key in T::nodes::<Indices<[usize; D]>, D>() {
        let key = key.expect("indices fit the schema depth");
        let a = json_core::get_by_key(before, key.as_ref(), left).map(|len| &left[..len]);
        let b = json_core::get_by_key(after, key.as_ref(), right).map(|len| &right[..len]);
        match (a, b) {
            (Ok(a), Ok(b)) if a == b => {}
            (
                Err(SerdeError::Value(ValueError::Absent)),
                Err(SerdeError::Value(ValueError::Absent)),
            ) => {}
            _ => emit(key, a, b)?,
        }
    }
    ControlFlow::Continue(())
}

// The error index is also the number of successfully applied entries.
fn apply<K: IntoKeys, P: AsRef<[u8]>>(
    tree: &mut impl TreeDeserializeOwned,
    patch: impl IntoIterator<Item = (K, P)>,
) -> Result<usize, (usize, SerdeError<de::Error>)> {
    let mut count = 0;
    for (key, payload) in patch {
        json_core::set_by_key(tree, key, payload.as_ref()).map_err(|error| (count, error))?;
        count += 1;
    }
    Ok(count)
}

struct Change {
    path: String,
    before: String,
    after: String,
}

fn main() {
    use miniconf::TreeSchema;

    let original = Settings::new();
    let mut desired = original.clone();
    desired.control.enabled = false;
    desired.control.mode = common::Mode::Standby;
    desired.output.dac[1] = 2048;
    desired.calibration.as_mut().unwrap().offset = -7;

    let mut changes = Vec::new();
    diff::<_, _, { Settings::SCHEMA.max_depth() }>(
        &original,
        &desired,
        [&mut [0; 32], &mut [0; 32]],
        |key, before, after| {
            let (Ok(before), Ok(after)) = (before, after) else {
                return ControlFlow::Break(key);
            };
            changes.push(Change {
                path: Settings::SCHEMA
                    .transcode::<Path<String>>(key.as_ref())
                    .unwrap()
                    .path,
                before: str::from_utf8(before).unwrap().into(),
                after: str::from_utf8(after).unwrap().into(),
            });
            ControlFlow::Continue(())
        },
    )
    .continue_value()
    .expect("all changed leaves must be readable");
    for change in &changes {
        println!("{}: {} -> {}", change.path, change.before, change.after);
    }

    let mut current = original.clone();
    assert_eq!(
        apply(
            &mut current,
            changes.iter().map(|c| (c.path.as_str(), &c.after))
        ),
        Ok(4)
    );
    assert!(current == desired);
    diff::<_, (), { Settings::SCHEMA.max_depth() }>(
        &current,
        &desired,
        [&mut [0; 32], &mut [0; 32]],
        |_, _, _| panic!("unexpected difference"),
    )
    .continue_value()
    .unwrap();

    apply(
        &mut current,
        changes.iter().rev().map(|c| (c.path.as_str(), &c.before)),
    )
    .unwrap();
    assert!(current == original);
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniconf::TreeSchema;

    #[test]
    fn round_trip() {
        main();
    }

    #[test]
    fn partial_apply() {
        let mut settings = Settings::new();
        let error = apply(
            &mut settings,
            [
                ("/control/enabled", "false"),
                ("/output/dac/0", "4096"),
                ("/output/dac/1", "2048"),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            (1, SerdeError::Value(ValueError::Access(_)))
        ));
        assert!(!settings.control.enabled);
        assert_eq!(settings.output.dac, [1024, 1024]);
        assert!(apply(&mut settings, [("/serial", "0")]).is_err());
    }

    #[test]
    fn diff_errors() {
        let before = Settings::new();
        let mut after = before.clone();
        after.calibration = None;
        let mut seen = 0;
        let outcome = diff::<_, (), { Settings::SCHEMA.max_depth() }>(
            &before,
            &after,
            [&mut [0; 32], &mut [0; 32]],
            |_, left, right| {
                assert!(left.is_ok());
                assert_eq!(right, Err(SerdeError::Value(ValueError::Absent)));
                seen += 1;
                ControlFlow::Continue(())
            },
        );
        assert_eq!(outcome, ControlFlow::Continue(()));
        assert_eq!(seen, 2); // Both calibration leaves are reported.
        let outcome = diff::<_, _, { Settings::SCHEMA.max_depth() }>(
            &before,
            &before,
            [&mut [0; 1], &mut [0; 1]],
            |key, left, right| {
                assert!(matches!(left, Err(SerdeError::Inner(_))));
                assert_eq!(left, right); // Equal failures are not equal values.
                ControlFlow::Break(key)
            },
        );
        assert_eq!(outcome.break_value().unwrap().as_ref(), [0]);
    }
}
