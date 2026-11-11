//! Smart shuffle ordering: artist adjacency and recency heuristics.
//!
//! Invariant: pure domain logic. No I/O, no clocks, no randomness. The caller
//! supplies an already randomized base permutation and the recent-play signal.

/// Number of most recent play records treated as "recent" when deprioritizing tracks.
pub const RECENT_PLAY_WINDOW: usize = 25;

/// Reorders `base` so consecutive entries avoid repeating an artist and recently
/// played tracks drift toward the end of the queue.
///
/// `artists` and `is_recent` are parallel to the track list, `base` holds indices
/// into them with the active track first. Mismatched shapes return `base`
/// unchanged so the caller can fall back to a plain shuffle.
pub fn smart_order(artists: &[&str], is_recent: &[bool], base: &[usize]) -> Vec<usize> {
    if artists.len() != is_recent.len() || base.len() != artists.len() {
        return base.to_vec();
    }
    let mut order = partition_unplayed(base, is_recent);
    repair_artist_repeats(&mut order, artists);
    order
}

fn partition_unplayed(base: &[usize], is_recent: &[bool]) -> Vec<usize> {
    let Some((&first, tail)) = base.split_first() else {
        return Vec::new();
    };
    let mut order = Vec::with_capacity(base.len());
    order.push(first);
    order.extend(tail.iter().copied().filter(|&idx| !is_recent_at(is_recent, idx)));
    order.extend(tail.iter().copied().filter(|&idx| is_recent_at(is_recent, idx)));
    order
}

fn is_recent_at(is_recent: &[bool], index: usize) -> bool {
    is_recent.get(index).copied().unwrap_or(false)
}

fn same_artist(left: usize, right: usize, artists: &[&str]) -> bool {
    match (artists.get(left), artists.get(right)) {
        (Some(a), Some(b)) => !a.is_empty() && a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

fn repair_artist_repeats(order: &mut [usize], artists: &[&str]) {
    for index in 1..order.len() {
        if !same_artist(order[index - 1], order[index], artists) {
            continue;
        }
        let Some(target) = find_swap_target(order, index, artists) else {
            continue;
        };
        order.swap(index, target);
    }
}

fn find_swap_target(order: &[usize], index: usize, artists: &[&str]) -> Option<usize> {
    if index == 0 || index >= order.len() {
        return None;
    }
    let previous = order[index - 1];
    (index + 1..order.len()).find(|&candidate| {
        !same_artist(previous, order[candidate], artists)
            && !repeats_next(order, candidate, artists)
    })
}

fn repeats_next(order: &[usize], index: usize, artists: &[&str]) -> bool {
    order.get(index + 1).is_some_and(|&next| same_artist(order[index], next, artists))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artists(list: &[&'static str]) -> Vec<&'static str> {
        list.to_vec()
    }

    #[test]
    fn test_smart_order_keeps_base_when_shapes_mismatch() {
        let names = artists(&["a", "b"]);
        let recent = [false];
        let base = [0, 1];
        assert_eq!(smart_order(&names, &recent, &base), vec![0, 1]);
    }

    #[test]
    fn test_smart_order_handles_empty_and_single_track() {
        assert_eq!(smart_order(&[], &[], &[]), Vec::<usize>::new());
        assert_eq!(smart_order(&["a"], &[false], &[0]), vec![0]);
    }

    #[test]
    fn test_smart_order_separates_consecutive_same_artist() {
        let names = artists(&["alpha", "alpha", "beta", "gamma"]);
        let recent = [false, false, false, false];
        let order = smart_order(&names, &recent, &[0, 1, 2, 3]);
        assert_eq!(order[0], 0, "active track must stay first");
        for pair in order.windows(2) {
            assert_ne!(names[pair[0]], names[pair[1]], "artists must not repeat: {order:?}");
        }
    }

    #[test]
    fn test_smart_order_matches_artist_case_insensitively() {
        let names = artists(&["Alpha", "alpha", "beta"]);
        let recent = [false, false, false];
        let order = smart_order(&names, &recent, &[0, 1, 2]);
        assert_ne!(order[0], order[1], "case-different artists are the same artist");
    }

    #[test]
    fn test_smart_order_moves_recent_tracks_to_the_back() {
        let names = artists(&["a", "b", "c", "d"]);
        let recent = [true, false, true, false];
        let order = smart_order(&names, &recent, &[0, 1, 2, 3]);
        assert_eq!(order[0], 0, "active track stays first even when recent");
        let tail = &order[1..];
        assert_eq!(tail, &[1, 3, 2], "unplayed tracks keep relative order before recent ones");
    }

    #[test]
    fn test_smart_order_single_artist_queue_degrades_without_panicking() {
        let names = artists(&["one", "one", "one"]);
        let recent = [false, false, false];
        let order = smart_order(&names, &recent, &[0, 2, 1]);
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], 0);
    }

    #[test]
    fn test_smart_order_is_a_permutation() {
        let names = artists(&["a", "b", "a", "c"]);
        let recent = [false, true, false, false];
        let mut order = smart_order(&names, &recent, &[0, 1, 2, 3]);
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2, 3]);
    }
}
