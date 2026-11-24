/**
 * Search in the calendar (S9, ADR 0246): a field in the calendar toolbar
 * over the core's `SearchPim` (events), with the results under it grouped
 * by upcoming (soonest first) and past (latest first). Choosing one shows
 * its day and opens its quick look. `/` focuses the field, as in Mail (the
 * `search.focus` action while the calendar is shown).
 *
 * Keyboard (combobox): Down / Up move through the results, Enter chooses,
 * Esc closes the results, then empties the field.
 *
 * Parts: `calendar.search`, `form.input` (`data-field="calendar-search"`),
 * `calendar.search.results` (listbox), `calendar.search.group` /
 * `.group.title`, `calendar.search.hit` (option; `.title`, `.when`,
 * `.detail`, `.dot`), `calendar.search.empty`.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import type { Calendar, PimSearchHit } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useUi } from "../../state/uiStore";
import { TYPING_PAUSE_MS, useDebounced } from "../../state/debounce";
import { useNow } from "../../state/clock";
import { anchorOf } from "../../widgets/Popover";
import { Icon } from "../../widgets/Icon";
import { Swatch, colorOf, geomId, useChangeContext } from "./common";
import { formatPimTime } from "./fmt";
import { usePimSearch } from "./queries";
import { dateString, pimTimeIn, rangeInstants, toCalendarDate, todayIn, toZoned } from "./time";
import { groupHits } from "./viewsModel";

/** The event id of a search hit (`null` for other kinds). */
function eventIdOf(hit: PimSearchHit): string | null {
  return hit.item.type === "event" ? hit.item.event_id : null;
}

export function CalendarSearch({
  tz,
  calendars,
}: {
  tz: string;
  calendars: readonly Calendar[] | undefined;
}) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const ids = { input: useId(), list: useId() };
  const inputRef = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const query = useDebounced(text.trim(), TYPING_PAUSE_MS);
  const results = usePimSearch(query);
  const todayStart = toZoned(todayIn(tz, useNow()), tz).toDate().getTime();
  const groups = groupHits(
    (results.data ?? []).filter((h) => eventIdOf(h) !== null),
    todayStart,
    tz,
  );
  const ordered = [...groups.upcoming, ...groups.past];
  const shown = open && query !== "" && text.trim() !== "";

  // `/` (`search.focus`) focuses the field; a request from before it showed does not count.
  const focusRequest = useUi((s) => s.focusRequest);
  const seenSeq = useRef(focusRequest?.seq ?? 0);
  useEffect(() => {
    if (focusRequest?.target !== "search" || focusRequest.seq === seenSeq.current) return;
    seenSeq.current = focusRequest.seq;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusRequest]);

  const choose = async (hit: PimSearchHit) => {
    const eventId = eventIdOf(hit);
    if (!hit.start || eventId === null) return;
    const day = toCalendarDate(pimTimeIn(hit.start, tz));
    const pim = ctx.pim.getState();
    pim.setAnchor(dateString(day));
    pim.setView("day");
    setOpen(false);
    const { instances } = await ctx.client.request(
      {
        type: "ListEventInstances",
        ...rangeInstants({ start: day, count: 1 }, tz),
        calendar_ids: null,
        tz,
      },
      "EventInstances",
    );
    const instance = instances.find((i) => i.event_id === eventId);
    if (!instance) return;
    // The card of the day view once it shows, else the field.
    requestAnimationFrame(() => {
      const card = document.querySelector(`[data-event="${CSS.escape(eventId)}"]`);
      const anchorEl = card ?? inputRef.current;
      if (anchorEl) ctx.pim.getState().setQuickLook({ instance, anchor: anchorOf(anchorEl) });
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setOpen(true);
        setActive((a) => Math.min(ordered.length - 1, a + 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((a) => Math.max(0, a - 1));
        break;
      case "Enter": {
        const hit = ordered[active];
        if (shown && hit) {
          event.preventDefault();
          void choose(hit);
        }
        break;
      }
      case "Escape":
        if (shown) {
          event.preventDefault();
          event.stopPropagation();
          setOpen(false);
        } else if (text !== "") {
          event.preventDefault();
          event.stopPropagation();
          setText("");
        }
        break;
    }
  };

  const optionId = (index: number) => `${ids.list}-${String(index)}`;
  const renderHit = (hit: PimSearchHit, index: number) => {
    const color = colorOf(calendars, hit.calendar_id ?? "", null);
    return (
      // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the combobox field handles the keys (Down / Up / Enter)
      <li
        key={`${eventIdOf(hit) ?? ""}-${String(index)}`}
        id={optionId(index)}
        role="option"
        data-part="calendar.search.hit"
        data-state={index === active ? "active" : undefined}
        aria-selected={index === active}
        onPointerDown={(e) => {
          // Keep the focus in the field until the choice is made.
          e.preventDefault();
        }}
        onClick={() => void choose(hit)}
      >
        <Swatch
          geom={geomId("search-dot", hit.calendar_id, index)}
          color={color}
          part="calendar.search.hit.dot"
        />
        <span data-part="calendar.search.hit.title">{hit.title || t("common-no-title")}</span>
        {hit.start && (
          <span data-part="calendar.search.hit.when">{formatPimTime(format, hit.start, tz)}</span>
        )}
        {hit.detail && <span data-part="calendar.search.hit.detail">{hit.detail}</span>}
      </li>
    );
  };

  return (
    <div data-part="calendar.search">
      <Icon name="search" part="calendar.search.icon" />
      <input
        ref={inputRef}
        id={ids.input}
        type="search"
        role="combobox"
        data-part="form.input"
        data-field="calendar-search"
        value={text}
        placeholder={t("pim-views-search-label")}
        aria-label={t("pim-views-search-label")}
        aria-expanded={shown}
        aria-controls={ids.list}
        aria-autocomplete="list"
        aria-activedescendant={shown && ordered[active] ? optionId(active) : undefined}
        onChange={(e) => {
          setText(e.target.value);
          setActive(0);
          setOpen(true);
        }}
        onFocus={() => {
          setOpen(true);
        }}
        onBlur={() => {
          setOpen(false);
        }}
        onKeyDown={onKeyDown}
      />
      <div
        id={ids.list}
        role="listbox"
        data-part="calendar.search.results"
        aria-label={t("pim-views-search-results")}
        hidden={!shown}
      >
        {shown && results.isSuccess && ordered.length === 0 && (
          <p data-part="calendar.search.empty">{t("pim-views-search-none", { query })}</p>
        )}
        {shown &&
          (
            [
              ["upcoming", groups.upcoming, 0],
              ["past", groups.past, groups.upcoming.length],
            ] as const
          ).map(([kind, hits, offset]) =>
            hits.length === 0 ? null : (
              <ul
                key={kind}
                role="group"
                data-part="calendar.search.group"
                data-group={kind}
                aria-label={t(`pim-views-search-${kind}`)}
              >
                <li role="presentation" data-part="calendar.search.group.title">
                  {t(`pim-views-search-${kind}`)}
                </li>
                {hits.map((hit, i) => renderHit(hit, offset + i))}
              </ul>
            ),
          )}
      </div>
    </div>
  );
}
