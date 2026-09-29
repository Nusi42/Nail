/**
 * The built-in widget registry: layout widget kinds → components. Each kind
 * and its options are also registered in nail-config's `WIDGETS` table.
 */
import type { WidgetRegistry } from "../layout/widgetRegistry";
import { AccountSettings } from "./AccountSettings";
import { AccountSetup } from "./AccountSetup";
import { AccountSwitcher } from "./AccountSwitcher";
import { Composer } from "./Composer";
import { FolderPicker } from "./FolderPicker";
import { FolderTree } from "./FolderTree";
import { MessageList } from "./MessageList";
import { MessageView } from "./MessageView";
import { Outbox } from "./Outbox";
import { SearchBar } from "./SearchBar";
import { Spacer } from "./Spacer";
import { StatusBar } from "./StatusBar";
import { Text } from "./Text";
import { ThreadView } from "./ThreadView";
import { Toolbar } from "./Toolbar";
import { VirtualAccountEditor } from "./VirtualAccountEditor";

export const builtinWidgets: WidgetRegistry = {
  "account-switcher": AccountSwitcher,
  "folder-tree": FolderTree,
  "message-list": MessageList,
  "message-view": MessageView,
  "thread-view": ThreadView,
  "search-bar": SearchBar,
  composer: Composer,
  outbox: Outbox,
  "folder-picker": FolderPicker,
  "account-setup": AccountSetup,
  "account-settings": AccountSettings,
  "virtual-account-editor": VirtualAccountEditor,
  toolbar: Toolbar,
  "status-bar": StatusBar,
  text: Text,
  spacer: Spacer,
};
