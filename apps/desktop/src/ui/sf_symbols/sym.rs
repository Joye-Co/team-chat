//! The app's icon concepts (NATIVE.md §6): one SF Symbol per concept, and the
//! sizes and weights each context draws them at.
//!
//! Views name a concept ([`Sym::Home`], [`Sym::Compose`] …), never a symbol
//! name or a Lucide icon. A new concept gets a row in NATIVE.md §6 first,
//! then a line in the `concepts!` table below.

use gpui::{Pixels, px};
use gpui_component::Icon;

use super::Weight;
use crate::ui::icons::Lucide;

macro_rules! concepts {
    ($($(#[$doc:meta])* $variant:ident = $sf:literal $(* $factor:literal)?;)*) => {
        /// An icon concept of the app, drawn as its SF Symbol.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Sym { $($(#[$doc])* $variant),* }

        impl Sym {
            /// Every concept, in table order.
            pub const ALL: &'static [Sym] = &[$(Sym::$variant),*];

            /// The system symbol name.
            pub fn name(self) -> &'static str {
                match self { $(Sym::$variant => $sf),* }
            }

            /// How large the symbol draws against its context's point size:
            /// 1, except for marks such as the live dot and for wide symbols
            /// scaled so their ink fits the same square as the others (an
            /// 18 tab slot: `person.2` 23 × 15 and `paintpalette` 21.5 × 17
            /// would stand out; review 2, R4).
            pub fn factor(self) -> f32 {
                match self { $(Sym::$variant => 1. $(* $factor)?),* }
            }
        }
    };
}

concepts! {
    // Destinations and the sidebar (§6, §9.1).
    /// 首页.
    Home = "tray";
    /// 产出.
    Outputs = "square.stack";
    /// 成员, and the 成员 settings tab.
    Members = "person.2" * 0.78;
    /// A 话题 that needs an icon without a state.
    Topic = "number";
    /// New topic, compose.
    Compose = "square.and.pencil";
    Search = "magnifyingglass";
    Filter = "line.3.horizontal.decrease";
    /// 单独问.
    AskAside = "bubble.left.and.text.bubble.right";
    PullRequest = "arrow.triangle.pull";
    Branch = "arrow.triangle.branch";
    Merge = "arrow.triangle.merge";
    /// Inspector toggle.
    Inspector = "sidebar.trailing";
    /// Sidebar toggle.
    SidebarToggle = "sidebar.leading";
    /// Attach, more input (the composer's leading button).
    Attach = "plus";
    Plus = "plus";
    Minus = "minus";
    /// Send: on an `accent` circle.
    Send = "arrow.up";
    // States (§6): the symbol is the state.
    /// Needs you (`amber`).
    NeedsYou = "exclamationmark.circle.fill";
    /// Live work (`blue`): a 6 pt dot at 13 pt.
    Live = "circle.fill" * 0.46;
    /// Done (`green`).
    Done = "checkmark.circle.fill";
    /// Problem (`red`).
    Problem = "exclamationmark.triangle.fill";
    /// Failed or stopped with an error (`red`).
    Failed = "xmark.circle.fill";
    /// A quiet alert in a row (`secondary`).
    Alert = "exclamationmark.circle";
    /// An outlined warning (gpui-component's notices).
    Warning = "exclamationmark.triangle";
    /// Waiting on someone.
    Waiting = "clock";
    /// Not started yet.
    Todo = "circle.dashed";
    Circle = "circle";
    Stopped = "nosign";
    Hourglass = "hourglass";
    // Machines (§6).
    /// A team computer.
    Computer = "display";
    Laptop = "laptopcomputer";
    /// Mac mini, and the 机器 settings tab.
    MacMini = "macmini";
    /// A Linux server (COMPLETENESS.md §2.19 machine rows).
    Server = "server.rack";
    /// Cloud runner.
    Cloud = "cloud";
    Offline = "wifi.slash";
    // v12/settings track: 账号 › 登录的设备 (NATIVE.md §8).
    /// A signed-in iPhone or iPad.
    Phone = "iphone";
    // Settings tabs (§8).
    /// 通用, settings.
    General = "gearshape";
    /// 账号.
    Account = "person.crop.circle";
    /// 通知.
    Bell = "bell";
    Muted = "bell.slash";
    BellBadge = "bell.badge";
    /// 外观.
    Appearance = "paintpalette" * 0.84;
    /// 快捷键.
    Shortcuts = "command";
    Keyboard = "keyboard";
    Privacy = "hand.raised";
    // Objects and actions.
    /// Lock, needs approval.
    Lock = "lock";
    Shield = "checkmark.shield";
    /// Open in new window.
    OpenWindow = "macwindow";
    More = "ellipsis";
    Check = "checkmark";
    Close = "xmark";
    ChevronDown = "chevron.down";
    ChevronUp = "chevron.up";
    ChevronLeft = "chevron.left";
    ChevronRight = "chevron.right";
    /// The first and the last page of a table footer (COMPLETENESS.md §2.7).
    PageFirst = "chevron.left.2";
    PageLast = "chevron.right.2";
    /// The pop-up button mark (§5.3).
    PopUp = "chevron.up.chevron.down";
    /// Open elsewhere: a trailing borderless button (§9.3).
    OpenExternal = "arrow.up.forward";
    Document = "doc.text";
    File = "doc";
    CodeFile = "doc.plaintext";
    MissingFile = "doc.questionmark";
    Diff = "plus.forwardslash.minus";
    Image = "photo";
    Images = "photo.on.rectangle";
    Copy = "doc.on.doc";
    /// The mark of an AI member without a face.
    AiMember = "sparkles";
    Person = "person";
    Invite = "person.badge.plus";
    Quote = "text.quote";
    List = "list.bullet";
    NumberedList = "list.number";
    Checklist = "checklist";
    Outline = "list.bullet.indent";
    Grid = "square.grid.2x2";
    Table = "tablecells";
    Download = "square.and.arrow.down";
    Upload = "square.and.arrow.up";
    Diagram = "point.3.connected.trianglepath.dotted";
    Video = "video";
    Audio = "waveform";
    Play = "play";
    Pause = "pause";
    Volume = "speaker.wave.2";
    PictureInPicture = "pip";
    ZoomIn = "plus.magnifyingglass";
    ZoomOut = "minus.magnifyingglass";
    Pointer = "cursorarrow";
    /// 实时查看.
    Watch = "eye";
    Hidden = "eye.slash";
    NewChat = "plus.bubble";
    Info = "info.circle";
    Help = "questionmark.circle";
    Return = "return";
    ReplyIndent = "arrow.turn.down.right";
    Reply = "arrowshape.turn.up.left";
    ArrowUp = "arrow.up";
    ArrowDown = "arrow.down";
    ArrowRight = "arrow.right";
    Sort = "arrow.up.arrow.down";
    Terminal = "terminal";
    Retry = "arrow.counterclockwise";
    Refresh = "arrow.clockwise";
    Undo = "arrow.uturn.backward";
    Link = "link";
    Code = "chevron.left.forwardslash.chevron.right";
    Mention = "at";
    TextFormat = "textformat";
    Bold = "bold";
    Italic = "italic";
    Strikethrough = "strikethrough";
    AlignLeft = "text.alignleft";
    Options = "slider.horizontal.3";
    Edit = "pencil";
    Expand = "arrow.up.left.and.arrow.down.right";
    Collapse = "arrow.down.right.and.arrow.up.left";
    SignOut = "rectangle.portrait.and.arrow.right";
    Emoji = "face.smiling";
    Chart = "chart.xyaxis.line";
    BarChart = "chart.bar";
    Activity = "waveform.path.ecg";
    Delete = "trash";
    Star = "star";
    StarFill = "star.fill";
    ScreenShare = "rectangle.inset.filled.and.person.filled";
    Scan = "viewfinder";
    Folder = "folder";
    Calendar = "calendar";
    Target = "scope";
    Mic = "mic";
    Book = "book";
    Square = "square";
    Asterisk = "asterisk";
    /// The busy spinner (gpui-component's `Spinner` turns it).
    Spinner = "progress.indicator";
    /// Find and replace (gpui-component's input search).
    Replace = "arrow.left.arrow.right";
    /// A bottom panel toggle (gpui-component's dock).
    BottomPanel = "rectangle.bottomthird.inset.filled";
    /// Not a concept: an icon that has none. Its name is empty, so it always
    /// draws the fallback (visible in debug builds).
    Missing = "";
}

/// The size and weight of a symbol in one context (§6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymSize {
    /// The point size AppKit draws the symbol at.
    pub point: f32,
    /// The square the symbol is centered in.
    pub slot: f32,
    pub weight: Weight,
    /// The larger side of the ink box in points, or 0 for the size AppKit
    /// draws at `point`. Set by [`Self::fill`].
    pub ink: f32,
}

impl SymSize {
    /// Sidebar rows: 15 pt in an 18 pt slot.
    pub const SIDEBAR: Self = Self::new(15., 18., Weight::Regular);
    /// Toolbar items: 16 pt Medium.
    pub const TOOLBAR: Self = Self::new(16., 20., Weight::Medium);
    /// Inline with 13 pt text.
    pub const INLINE: Self = Self::new(13., 16., Weight::Regular);
    /// Inline with 11 pt text.
    pub const INLINE_SMALL: Self = Self::new(11., 14., Weight::Regular);
    /// Settings toolbar tabs: every symbol's ink fills the same 18 square
    /// (§2.19), whatever its proportions (review 3 K-06).
    pub const TAB: Self = Self::new(18., 24., Weight::Regular).fill(18.);
    /// The pop-up and pull-down mark (§5.3): 9 pt.
    pub const POPUP: Self = Self::new(9., 12., Weight::Semibold);
    /// The contexts the app draws at, prewarmed at launch.
    pub const CONTEXTS: [Self; 6] = [
        Self::SIDEBAR,
        Self::TOOLBAR,
        Self::INLINE,
        Self::INLINE_SMALL,
        Self::TAB,
        Self::POPUP,
    ];

    pub const fn new(point: f32, slot: f32, weight: Weight) -> Self {
        Self {
            point,
            slot,
            weight,
            ink: 0.,
        }
    }

    /// The point size is chosen per symbol so that the larger side of its
    /// ink box is `ink` points: one square for a row of unlike symbols.
    pub const fn fill(mut self, ink: f32) -> Self {
        self.ink = ink;
        self
    }

    /// The §6 context for an icon slot of the older Lucide sizes: 18 is a
    /// sidebar row, 20 the toolbar, 16 inline with 13 pt text, 14 inline
    /// with 11 pt text, 24 a settings tab; others keep 13/16 of the slot.
    pub fn for_slot(slot: Pixels) -> Self {
        let s = f32::from(slot);
        match (s * 2.).round() as u32 {
            36 => Self::SIDEBAR,
            40 => Self::TOOLBAR,
            32 => Self::INLINE,
            28 => Self::INLINE_SMALL,
            48 => Self::new(18., 24., Weight::Regular),
            _ => Self::new((s * 13. / 16. * 4.).round() / 4., s, Weight::Regular),
        }
    }

    /// The same context with another weight.
    pub const fn weight(mut self, weight: Weight) -> Self {
        self.weight = weight;
        self
    }

    pub fn slot_px(self) -> Pixels {
        px(self.slot)
    }
}

impl Sym {
    /// The symbol at a context size, in the text color of its parent.
    pub fn icon(self, size: SymSize) -> Icon {
        super::asset::icon(self, size)
    }

    /// Sidebar row (15 pt in 18).
    pub fn sidebar(self) -> Icon {
        self.icon(SymSize::SIDEBAR)
    }

    /// Toolbar item (16 pt Medium).
    pub fn toolbar(self) -> Icon {
        self.icon(SymSize::TOOLBAR)
    }

    /// Inline with 13 pt text.
    pub fn inline(self) -> Icon {
        self.icon(SymSize::INLINE)
    }

    /// Inline with 11 pt text.
    pub fn inline_small(self) -> Icon {
        self.icon(SymSize::INLINE_SMALL)
    }

    /// Settings toolbar tab (18 pt).
    pub fn tab(self) -> Icon {
        self.icon(SymSize::TAB)
    }
}

/// gpui-component sizes the icons it is given (button, menu item): the
/// symbol comes at the inline size and scales with the element.
impl From<Sym> for Icon {
    fn from(sym: Sym) -> Self {
        super::asset::icon_unsized(sym)
    }
}

/// A concept as a child element: the inline size, in the parent's text
/// color (like `.child(IconName::…)` before).
impl gpui::IntoElement for Sym {
    type Element = <Icon as gpui::IntoElement>::Element;

    fn into_element(self) -> Self::Element {
        self.inline().into_element()
    }
}

/// Older call sites (and other tracks) still name Lucide icons: each maps to
/// its concept. An icon without a concept draws the dev fallback.
#[allow(deprecated)]
impl From<Lucide> for Sym {
    fn from(l: Lucide) -> Self {
        from_lucide(l).unwrap_or(Sym::Missing)
    }
}

macro_rules! lucide {
    ($($lucide:ident => $sym:ident,)*) => {
        /// The concept of a Lucide icon the app (or gpui-component) used.
        pub fn from_lucide(l: Lucide) -> Option<Sym> {
            match l {
                $(Lucide::$lucide => Some(Sym::$sym),)*
                _ => None,
            }
        }

        /// The Lucide icons that have a concept, for the asset override.
        pub const LUCIDE: &[(Lucide, Sym)] = &[$((Lucide::$lucide, Sym::$sym)),*];
    };
}

lucide! {
    House => Home,
    Inbox => Home,
    Layers => Outputs,
    Users => Members,
    UsersRound => Members,
    Hash => Topic,
    SquarePen => Compose,
    Search => Search,
    SearchX => Search,
    MessageCircle => AskAside,
    GitPullRequest => PullRequest,
    GitBranch => Branch,
    GitMerge => Merge,
    PanelRight => Inspector,
    PanelRightOpen => Inspector,
    PanelRightClose => Inspector,
    PanelLeft => SidebarToggle,
    PanelLeftOpen => SidebarToggle,
    PanelLeftClose => SidebarToggle,
    Paperclip => Attach,
    Plus => Plus,
    Minus => Minus,
    Send => Send,
    CircleDot => Live,
    CircleCheck => Done,
    TriangleAlert => Warning,
    CircleX => Failed,
    CircleAlert => Alert,
    Clock => Waiting,
    CircleDashed => Todo,
    Circle => Circle,
    Ban => Stopped,
    Hourglass => Hourglass,
    Monitor => Computer,
    Laptop => Laptop,
    Server => Server,
    WifiOff => Offline,
    Settings => General,
    Bell => Bell,
    BellOff => Muted,
    BellRing => BellBadge,
    Palette => Appearance,
    Command => Shortcuts,
    Keyboard => Keyboard,
    Hand => Privacy,
    Lock => Lock,
    Shield => Shield,
    AppWindow => OpenWindow,
    Ellipsis => More,
    Check => Check,
    X => Close,
    Close => Close,
    ChevronDown => ChevronDown,
    ChevronUp => ChevronUp,
    ChevronLeft => ChevronLeft,
    ChevronRight => ChevronRight,
    ChevronsUpDown => PopUp,
    ExternalLink => OpenExternal,
    ArrowUpRight => OpenExternal,
    FileText => Document,
    File => File,
    FileCode => CodeFile,
    FileX => MissingFile,
    FileDiff => Diff,
    Image => Image,
    FileImage => Image,
    Images => Images,
    Copy => Copy,
    Bot => AiMember,
    User => Person,
    UserPlus => Invite,
    TextQuote => Quote,
    Quote => Quote,
    List => List,
    ListOrdered => NumberedList,
    ListChecks => Checklist,
    ListTree => Outline,
    Table => Table,
    Download => Download,
    Upload => Upload,
    Workflow => Diagram,
    Video => Video,
    AudioLines => Audio,
    Play => Play,
    Pause => Pause,
    Volume2 => Volume,
    PictureInPicture2 => PictureInPicture,
    ZoomIn => ZoomIn,
    ZoomOut => ZoomOut,
    MousePointer2 => Pointer,
    Eye => Watch,
    EyeOff => Hidden,
    MessageSquarePlus => NewChat,
    Info => Info,
    CircleQuestionMark => Help,
    CornerDownLeft => Return,
    CornerDownRight => ReplyIndent,
    Reply => Reply,
    ArrowUp => ArrowUp,
    ArrowDown => ArrowDown,
    ArrowRight => ArrowRight,
    ArrowUpDown => Sort,
    SortAscending => ArrowUp,
    SortDescending => ArrowDown,
    Terminal => Terminal,
    SquareTerminal => Terminal,
    RotateCcw => Retry,
    RotateCw => Refresh,
    RefreshCw => Refresh,
    Undo2 => Undo,
    Link => Link,
    Code => Code,
    AtSign => Mention,
    Type => TextFormat,
    Bold => Bold,
    Italic => Italic,
    Strikethrough => Strikethrough,
    TextAlignStart => AlignLeft,
    SlidersHorizontal => Options,
    Pencil => Edit,
    Maximize => Expand,
    Maximize2 => Expand,
    Minimize2 => Collapse,
    LogOut => SignOut,
    FaceSlightlySmiling => Emoji,
    FaceSlightlySmilingPlus => Emoji,
    ChartLine => Chart,
    ChartColumn => BarChart,
    Activity => Activity,
    Trash => Delete,
    Star => Star,
    StarFill => StarFill,
    ScreenShare => ScreenShare,
    Scan => Scan,
    FolderOpen => Folder,
    Calendar => Calendar,
    Target => Target,
    Mic => Mic,
    BookOpen => Book,
    Square => Square,
    Asterisk => Asterisk,
    Loader => Spinner,
    Minimize => Collapse,
    Inspector => Inspector,
    CaseSensitive => TextFormat,
    Replace => Replace,
    PanelBottom => BottomPanel,
    PanelBottomOpen => BottomPanel,
    LoaderCircle => Spinner,
}
