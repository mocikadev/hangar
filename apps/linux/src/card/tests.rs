use super::*;

// Called on the existing isolated GTK test's single main thread.
pub(crate) fn verify_card_states(mut card: Card) {
    let widget = CardWidgets::new(&card.account.id, Rc::new(|_| {}));
    card.account.current = true;
    card.account.stale = false;
    card.recommended = false;
    widget.update(&card, false, None);
    assert!(widget.body.has_css_class("current-account"));
    assert!(widget.badges.text().contains("使用中"));
    assert!(!widget.switch.is_visible());
    assert!(!widget.delete.get_visible());
    assert!(widget.reauth.get_visible());
    assert!(widget.more.is_visible());
    assert!(!widget.state.text().contains("更新于 未知"));

    card.account.current = false;
    card.quota = None;
    card.state = QuotaState::Failed;
    widget.update(&card, false, None);
    assert!(!widget.body.has_css_class("current-account"));
    assert!(widget.badges.text().is_empty());
    assert_eq!(widget.plan.text(), "套餐未知");
    assert_eq!(widget.state.text(), "额度获取失败");
    assert_eq!(widget.remaining.text(), "未知");
    assert!(!widget.progress.is_visible());
    assert!(widget.switch.is_sensitive());

    card.account.stale = true;
    card.recommended = true;
    widget.update(&card, false, None);
    assert!(!widget.quota.is_visible());
    assert!(!widget.badges.text().contains("推荐"));
    assert_eq!(widget.state.text(), "需要重新登录");
    assert_eq!(widget.switch.label().as_deref(), Some("重新登录"));
    assert!(!widget.reauth.get_visible());
    assert!(widget.delete.get_visible());
    card.account.current = true;
    widget.update(&card, false, None);
    assert!(!widget.more.is_visible());
    assert!(widget.switch.is_sensitive());

    card.account.stale = false;
    card.account.current = false;
    for operation in [Operation::Switch, Operation::Delete, Operation::Reauth] {
        widget.update(&card, true, Some(operation));
        assert!(widget.operation.is_visible());
        assert!(!widget.switch.is_sensitive());
        assert!(!widget.reauth.get_sensitive());
        assert!(!widget.delete.get_sensitive());
    }
    widget.update(&card, false, None);
    assert!(!widget.operation.is_visible());
    assert!(widget.switch.is_sensitive());
}

// Also verify controller routing: only the target card receives operation feedback.
pub(crate) fn assert_operation_visible(widget: &CardWidgets, expected: bool) {
    assert_eq!(widget.operation.get_visible(), expected);
}
