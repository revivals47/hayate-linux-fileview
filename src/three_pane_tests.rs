use super::*;

fn hit_divider_logic(x: f32, y: f32, sw: f32, lw: f32, pw: f32) -> Option<DragTarget> {
    if y < BREADCRUMB_HEIGHT { return None; }
    if sw > 0.0 && (x - sw).abs() < DIVIDER_HIT { return Some(DragTarget::SidebarRight); }
    let px = sw + lw;
    if pw > 0.0 && (x - px).abs() < DIVIDER_HIT { return Some(DragTarget::PreviewLeft); }
    None
}

#[test]
fn divider_sidebar_edge() {
    assert!(matches!(hit_divider_logic(151.0, 30.0, 150.0, 300.0, 200.0), Some(DragTarget::SidebarRight)));
    assert!(hit_divider_logic(140.0, 30.0, 150.0, 300.0, 200.0).is_none());
}

#[test]
fn divider_preview_edge() {
    assert!(matches!(hit_divider_logic(451.0, 30.0, 150.0, 300.0, 200.0), Some(DragTarget::PreviewLeft)));
}

#[test]
fn divider_above_breadcrumb() {
    assert!(hit_divider_logic(150.0, 10.0, 150.0, 300.0, 200.0).is_none());
}

#[test]
fn divider_no_sidebar() {
    assert!(hit_divider_logic(0.0, 30.0, 0.0, 500.0, 200.0).is_none());
}

#[test]
fn focus_cycle_forward() {
    assert_eq!(next_focus_logic(PaneFocus::FileList, true, true, false), PaneFocus::Preview);
    assert_eq!(next_focus_logic(PaneFocus::Preview, true, true, false), PaneFocus::Sidebar);
    assert_eq!(next_focus_logic(PaneFocus::Sidebar, true, true, false), PaneFocus::FileList);
}

#[test]
fn focus_cycle_reverse() {
    assert_eq!(next_focus_logic(PaneFocus::FileList, true, true, true), PaneFocus::Sidebar);
}

#[test]
fn focus_skip_hidden() {
    assert_eq!(next_focus_logic(PaneFocus::FileList, false, true, false), PaneFocus::Preview);
    assert_eq!(next_focus_logic(PaneFocus::Preview, false, true, false), PaneFocus::FileList);
}

#[test]
fn focus_only_filelist() {
    assert_eq!(next_focus_logic(PaneFocus::FileList, false, false, false), PaneFocus::FileList);
}
