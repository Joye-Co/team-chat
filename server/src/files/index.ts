// Files, 产出, avatars, diagnostics, releases and updates. See docs/protocol.md, V1 extensions.
import "./room";
import "./bridge";
import "./page-log";
import { registerOpsRoutes } from "./ops";
import { registerFileRoutes } from "./routes";

registerFileRoutes();
registerOpsRoutes();
