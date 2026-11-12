//! Incremental model updates. Replacing a model makes every list view drop and
//! recreate its visible rows; updating it in place re-renders only what changed.
use slint::{Model, ModelRc, VecModel};

/// Brings `model` to `rows` while notifying only the rows that differ, so the
/// list keeps its instantiated delegates when a page is appended or one entry
/// changes. A listing that is mostly different (another folder, a new sort)
/// is replaced in one reset, which is cheaper than one notification per row.
pub fn sync_rows<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    let old = model.row_count();
    let overlap = old.min(rows.len());
    let removed = old - overlap;
    if overlap == 0 || removed > 32 {
        model.set_vec(rows);
        return;
    }
    let changed: Vec<usize> = (0..overlap)
        .filter(|at| model.row_data(*at).as_ref() != Some(&rows[*at]))
        .collect();
    if changed.len() * 2 > overlap {
        model.set_vec(rows);
        return;
    }
    let mut rows = rows;
    let extra = rows.split_off(overlap);
    for at in changed {
        model.set_row_data(at, rows[at].clone());
    }
    for at in (overlap..old).rev() {
        model.remove(at);
    }
    if !extra.is_empty() {
        model.extend(extra);
    }
}

/// Updates the model behind a property in place when it is a `VecModel`,
/// otherwise installs a new one with `set`.
pub fn update<T: Clone + PartialEq + 'static>(
    current: ModelRc<T>,
    rows: Vec<T>,
    set: impl FnOnce(ModelRc<T>),
) {
    match current.as_any().downcast_ref::<VecModel<T>>() {
        Some(model) => sync_rows(model, rows),
        None => set(ModelRc::new(VecModel::from(rows))),
    }
}

#[cfg(test)]
mod tests {
    use super::{sync_rows, update};
    use slint::{Model, ModelRc, VecModel};

    #[test]
    fn sync_rows_reaches_the_target() {
        for (old, new) in [
            (vec![], vec![1, 2, 3]),
            (vec![1, 2, 3], vec![]),
            (vec![1, 2, 3], vec![1, 2, 3, 4, 5]),
            (vec![1, 2, 3, 4, 5], vec![1, 2, 3]),
            (vec![1, 2, 3, 4], vec![1, 9, 3, 4]),
            (vec![1, 2, 3, 4], vec![4, 3, 2, 1]),
            ((0..100).collect(), (0..10).collect()),
            ((0..10).collect(), (0..100).collect()),
        ] {
            let model = VecModel::from(old);
            sync_rows(&model, new.clone());
            assert_eq!(model.iter().collect::<Vec<i32>>(), new);
        }
    }

    #[test]
    fn update_keeps_a_vec_model_and_replaces_anything_else() {
        let shared = std::rc::Rc::new(VecModel::from(vec![1, 2]));
        let mut replaced = false;
        update(ModelRc::from(shared.clone()), vec![1, 2, 3], |_| {
            replaced = true
        });
        assert!(!replaced);
        assert_eq!(shared.iter().collect::<Vec<i32>>(), vec![1, 2, 3]);
        let mut installed = Vec::new();
        update(ModelRc::<i32>::default(), vec![7], |model| {
            installed = model.iter().collect()
        });
        assert_eq!(installed, vec![7]);
    }
}
