use std::sync::atomic::{AtomicU8, Ordering};

const LOCKED: u8 = 0;
const SHORTCUT: u8 = 1;
const HOVER: u8 = 2;
const WAITING_FOR_CURSOR_LEAVE: u8 = 3;

#[derive(Default)]
pub struct OverlayMoveState {
    activation: AtomicU8,
}

impl OverlayMoveState {
    pub fn is_enabled(&self) -> bool {
        matches!(self.activation.load(Ordering::SeqCst), SHORTCUT | HOVER)
    }

    pub fn is_hover_armed(&self) -> bool {
        self.activation.load(Ordering::SeqCst) == HOVER
    }

    /// 切换移动模式，并返回切换后的状态。
    pub fn toggle(&self) -> bool {
        let previous = self
            .activation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(if matches!(current, LOCKED | WAITING_FOR_CURSOR_LEAVE) {
                    SHORTCUT
                } else {
                    LOCKED
                })
            })
            .unwrap_or(LOCKED);
        previous == LOCKED
    }

    /// 仅从锁定状态进入悬停解锁，避免覆盖用户主动开启的快捷键模式。
    pub fn arm_from_hover(&self) -> bool {
        self.activation
            .compare_exchange(LOCKED, HOVER, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// 只关闭由悬停触发的移动模式，不干扰快捷键模式。
    pub fn lock_hover(&self) -> bool {
        self.activation
            .compare_exchange(HOVER, LOCKED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// 拖动完成后无条件恢复锁定，并返回此前是否处于可移动状态。
    pub fn lock_after_drag(&self) -> bool {
        matches!(
            self.activation
                .swap(WAITING_FOR_CURSOR_LEAVE, Ordering::SeqCst),
            SHORTCUT | HOVER
        )
    }

    /// 鼠标离开交互区后解除冷却，允许下一次完整悬停。
    pub fn reset_after_cursor_leave(&self) {
        let _ = self.activation.compare_exchange(
            WAITING_FOR_CURSOR_LEAVE,
            LOCKED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::OverlayMoveState;

    #[test]
    fn shortcut_toggles_between_click_through_and_drag_enabled() {
        let state = OverlayMoveState::default();

        assert!(!state.is_enabled());
        assert!(state.toggle());
        assert!(state.is_enabled());
        assert!(!state.toggle());
        assert!(!state.is_enabled());
    }

    #[test]
    fn two_second_hover_can_arm_dragging_and_leaving_locks_it_again() {
        let state = OverlayMoveState::default();

        assert!(state.arm_from_hover());
        assert!(state.is_enabled());
        assert!(state.is_hover_armed());

        assert!(state.lock_hover());
        assert!(!state.is_enabled());
        assert!(!state.is_hover_armed());
    }

    #[test]
    fn finishing_a_drag_always_restores_click_through() {
        let state = OverlayMoveState::default();
        assert!(state.arm_from_hover());

        assert!(state.lock_after_drag());
        assert!(!state.is_enabled());
        assert!(!state.arm_from_hover());

        state.reset_after_cursor_leave();
        assert!(state.arm_from_hover());
    }

    #[test]
    fn leaving_hover_does_not_cancel_shortcut_mode() {
        let state = OverlayMoveState::default();
        assert!(state.toggle());

        assert!(!state.arm_from_hover());
        assert!(!state.lock_hover());
        assert!(state.is_enabled());
    }
}
