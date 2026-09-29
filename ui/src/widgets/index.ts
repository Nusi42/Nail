/**
 * The M0 widget registry: layout widget kinds → components.
 */
import type { WidgetRegistry } from "../layout/widgetRegistry";
import { AccountSwitcher } from "./AccountSwitcher";
import { FolderTree } from "./FolderTree";
import { MessageList } from "./MessageList";
import { MessageView } from "./MessageView";
import { Spacer } from "./Spacer";
import { StatusBar } from "./StatusBar";
import { Text } from "./Text";
import { Toolbar } from "./Toolbar";

export const builtinWidgets: WidgetRegistry = {
  "account-switcher": AccountSwitcher,
  "folder-tree": FolderTree,
  "message-list": MessageList,
  "message-view": MessageView,
  toolbar: Toolbar,
  "status-bar": StatusBar,
  text: Text,
  spacer: Spacer,
};
