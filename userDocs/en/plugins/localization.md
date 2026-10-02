# Translations

A plug-in brings its own texts in [Fluent](https://projectfluent.org)
catalogs, one per language, named in the manifest:

```json
"contributes": { "locales": { "en": "locales/en.ftl", "de": "locales/de.ftl" } }
```

```ftl
# locales/en.ftl
follow-up-title = Follow up…
created = Follow-up task created, due { $due }
items = { $count ->
    [one] One follow-up
   *[other] { $count } follow-ups
}
```

The same catalogs serve two purposes:

- **Manifest texts** written as `%id%` (`name`, `description`, every
  `title` and `description` of a contribution — commands, panels,
  composer actions, Markdown blocks and their `template`, attachment
  previews, attachment and link actions, jobs, MCP tool descriptions —
  and setting labels) are shown in Nail's language.
- **Texts in code** come from `nail.l10n`:

  ```ts
  nail.l10n.locale;                          // "de"
  nail.l10n.t("created", { due: "2026-10-12" });
  nail.l10n.has("items");                    // true
  ```

Nail looks a message up in the catalog of its language (`pt-BR`, then
`pt`), then in English, and shows the id itself when neither has it —
message by message, so a partial translation still works. Arguments are
strings or numbers; select expressions pick plural forms (`[one]`,
`[other]`, …) by the number.

Use `nail.l10n` for every text the user sees — dialog titles and labels,
notifications, veto reasons, banners and badges, preview texts. Language
data that is not shown can live there too: the `attachment-check` example
keeps its keyword list per language in its catalogs.

`nail.l10n` needs no permission. The SDK's test host reads catalogs too:
`createTestHost({ locale: "de", catalogs: loadCatalogs(dir) })` with
`loadCatalogs` from `@nail/plugin-api/tools`.

Texts that are not for the user stay in English: log lines, the
`description` of tools for AI agents, error messages you throw.
