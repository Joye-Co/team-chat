//! The menu bar item (NSStatusItem through objc2; GPUI has no API for it).
//! A 16 pt template of the icon's brackets and circle; a dot at the top right
//! when something waits on you (DESIGN.md §13.16). A click opens the main window.
//! AppKit calls the click target on the main thread inside GPUI's run loop;
//! the target only sends on a channel that a GPUI task drains.

use futures::{
    StreamExt as _,
    channel::mpsc::{UnboundedSender, unbounded},
};
use gpui::{App, Global};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, runtime::AnyObject, sel,
};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSSize, NSString};

/// The brand's template images (assets/brand/menubar), drawn at 2x and shown at 16 pt.
const ICON: &[u8] = include_bytes!("../../../../../assets/brand/menubar/StatusItemTemplate@2x.png");
const ICON_WAITING: &[u8] =
    include_bytes!("../../../../../assets/brand/menubar/StatusItemWaitingTemplate@2x.png");

fn template_image(png: &[u8], description: &NSString) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(png);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setSize(NSSize::new(16., 16.));
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(description));
    Some(image)
}

use crate::{
    app::{Changes, Services},
    copy::{fill, nav},
    data::{Change, ChatProvider, SessionState},
};

struct TargetIvars {
    clicks: UnboundedSender<()>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DesktopNavStatusItemTarget"]
    #[ivars = TargetIvars]
    struct ClickTarget;

    impl ClickTarget {
        #[unsafe(method(statusItemClicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            let _ = self.ivars().clicks.unbounded_send(());
        }
    }

    unsafe impl NSObjectProtocol for ClickTarget {}
);

impl ClickTarget {
    fn new(clicks: UnboundedSender<()>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { clicks });
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// Keeps the item and its target alive for the life of the app.
struct StatusItem {
    item: Retained<NSStatusItem>,
    _target: Retained<ClickTarget>,
    shown: Option<(u32, u32)>,
}

impl Global for StatusItem {}

/// Unread and mention totals over the groups that are not muted.
pub fn totals(provider: &dyn ChatProvider) -> (u32, u32) {
    if !matches!(provider.session(), SessionState::SignedIn { .. }) {
        return (0, 0);
    }
    provider
        .groups()
        .iter()
        .filter(|g| !provider.group_muted(&g.id))
        .fold((0, 0), |(u, m), g| {
            (u.saturating_add(g.unread), m.saturating_add(g.mentions))
        })
}

pub fn install(services: Services, cx: &mut App) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (tx, mut rx) = unbounded();
    let target = ClickTarget::new(tx, mtm);
    let bar = NSStatusBar::systemStatusBar();
    let item = bar.statusItemWithLength(NSVariableStatusItemLength);
    let Some(button) = item.button(mtm) else {
        bar.removeStatusItem(&item);
        return;
    };
    // SAFETY: the target outlives the button (both are kept in the global).
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(statusItemClicked:)));
    }
    cx.set_global(StatusItem {
        item,
        _target: target,
        shown: None,
    });
    update(&services, cx);

    cx.spawn(async move |cx| {
        while rx.next().await.is_some() {
            cx.update(super::show_main_window);
        }
    })
    .detach();

    let watched = services.clone();
    cx.subscribe(&services.hub, move |_, changes: &Changes, cx| {
        if changes.any(|c| matches!(c, Change::Groups | Change::Session)) {
            update(&watched, cx);
        }
    })
    .detach();
}

fn update(services: &Services, cx: &mut App) {
    let (unread, mentions) = totals(services.provider.as_ref());
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let state = cx.global_mut::<StatusItem>();
    if state.shown == Some((unread, mentions)) {
        return;
    }
    state.shown = Some((unread, mentions));
    let Some(button) = state.item.button(mtm) else {
        return;
    };
    let tooltip = if mentions > 0 {
        fill(nav::STATUS_ITEM_MENTIONS, &[("n", &mentions.to_string())])
    } else if unread > 0 {
        fill(nav::STATUS_ITEM_UNREAD, &[("n", &unread.to_string())])
    } else {
        nav::STATUS_ITEM_NONE.to_string()
    };
    let tooltip = NSString::from_str(&tooltip);
    // The dot says that something waits; the tooltip says what.
    let png = if unread > 0 || mentions > 0 {
        ICON_WAITING
    } else {
        ICON
    };
    let image = template_image(png, &tooltip);
    button.setImage(image.as_deref());
    button.setTitle(&NSString::from_str(""));
    button.setToolTip(Some(&tooltip));
}

/// For the self-test: the item exists and its tooltip matches the totals.
pub fn check(services: &Services, cx: &App) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(state) = cx.try_global::<StatusItem>() else {
        return false;
    };
    state.shown == Some(totals(services.provider.as_ref())) && state.item.button(mtm).is_some()
}
