/**
 * The room finder of the event editor (mount point `RoomFinder`; ADR 0072
 * rooms and room lists): for an event in a calendar of an Exchange account,
 * "Find a room" lists the organization's rooms — all, or one room list —
 * with their seats, building / floor, accessibility, equipment and whether
 * they are free for the event's time (`FindRooms`, free/busy through the
 * account). Adding a room books it as a room attendee (and fills an empty
 * location with its name); booked rooms are listed with "Remove". Without
 * the `Place.Read.All` permission the core's warning is shown.
 */
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { Attendee, BusyPeriod, PimTime, RoomAvailability } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useCalendars } from "../pim/queries";
import { formatInstant } from "../pim/fmt";
import { pimTimeIn, utcString } from "../pim/time";
import { Problem } from "./common";
import { addRoom, isRoom, removeRoom, roomState } from "./model";
import { useExchangeAccount, useRoomLists, useRooms } from "./queries";

export interface RoomFinderProps {
  /** The event's calendar (its account decides whether rooms can be found). */
  calendarId: string | null;
  /** Use this Exchange account instead of the calendar's. */
  accountId?: string | null;
  start: PimTime;
  end: PimTime;
  attendees: readonly Attendee[];
  onAttendeesChange: (attendees: Attendee[]) => void;
  location: string;
  onLocationChange: (location: string) => void;
  /** The viewer's zone (times of busy periods). */
  tz: string;
}

/** The UTC range of the event as `FindRooms` wants it (whole days for all-day events). */
export function eventRange(start: PimTime, end: PimTime, tz: string): { from: string; to: string } {
  const from = pimTimeIn(start, tz);
  let to = pimTimeIn(end, tz);
  if (end.type === "date" && to.compare(from) <= 0) to = from.add({ days: 1 });
  return { from: utcString(from), to: utcString(to) };
}

function Busy({ periods, tz }: { periods: readonly BusyPeriod[]; tz: string }) {
  const t = useT();
  const format = useDateFormat();
  const clock = (utc: string) => formatInstant(format, utc, tz);
  return (
    <ul data-part="exchange.room.busy" aria-label={t("exchange-room-busy-label")}>
      {periods.map((p) => (
        <li key={`${p.start}-${p.end}`} data-part="exchange.room.busy.item" data-kind={p.kind}>
          {t("exchange-room-busy-period", { start: clock(p.start), end: clock(p.end) })}
        </li>
      ))}
    </ul>
  );
}

function RoomRow({
  availability,
  booked,
  tz,
  onAdd,
  onRemove,
}: {
  availability: RoomAvailability;
  booked: boolean;
  tz: string;
  onAdd: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const { room } = availability;
  const state = roomState(availability);
  const place = [room.building, room.floor ? t("exchange-room-floor", { floor: room.floor }) : null]
    .filter((x): x is string => x !== null && x !== "")
    .join(", ");
  return (
    <li
      data-part="exchange.room"
      data-availability={state}
      data-state={booked ? "selected" : undefined}
    >
      <span data-part="exchange.room.name">{room.name}</span>
      {room.capacity !== null && (
        <span data-part="exchange.room.capacity">
          {t("exchange-room-capacity", { count: room.capacity })}
        </span>
      )}
      {place && <span data-part="exchange.room.place">{place}</span>}
      {room.accessible === true && (
        <span data-part="exchange.room.accessible">{t("exchange-room-accessible")}</span>
      )}
      {room.features.length > 0 && (
        <ul data-part="exchange.room.features" aria-label={t("exchange-room-features")}>
          {room.features.map((f) => (
            <li key={f} data-part="exchange.room.feature">
              {f}
            </li>
          ))}
        </ul>
      )}
      <span data-part="exchange.room.availability" data-availability={state}>
        {t(`exchange-room-${state}`)}
      </span>
      {state === "busy" && availability.periods.length > 0 && (
        <Busy periods={availability.periods} tz={tz} />
      )}
      {booked ? (
        <button
          type="button"
          data-part="form.button"
          data-action="remove-room"
          aria-label={t("exchange-room-remove-named", { name: room.name })}
          onClick={onRemove}
        >
          {t("exchange-room-remove")}
        </button>
      ) : (
        <button
          type="button"
          data-part="form.button"
          data-action="add-room"
          aria-label={t("exchange-room-add-named", { name: room.name })}
          onClick={onAdd}
        >
          {t("exchange-room-add")}
        </button>
      )}
    </li>
  );
}

/** Mount point `RoomFinder` (event editor). Renders nothing outside Exchange calendars. */
export function RoomFinder(props: RoomFinderProps) {
  const calendars = useCalendars();
  const calendar = calendars.data?.find((c) => c.id === props.calendarId);
  const account = useExchangeAccount(
    props.accountId ?? (calendar?.exchange ? (calendar.account_id ?? null) : null),
  );
  if (!account) return null;
  return <Finder {...props} accountId={account.id} />;
}

function Finder({
  accountId,
  start,
  end,
  attendees,
  onAttendeesChange,
  location,
  onLocationChange,
  tz,
}: RoomFinderProps & { accountId: string }) {
  const t = useT();
  const ids = { panel: useId(), list: useId(), capacity: useId() };
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<string>("");
  const [capacity, setCapacity] = useState<string>("");
  const [onlyFree, setOnlyFree] = useState(false);
  const range = eventRange(start, end, tz);
  const valid = Date.parse(range.to) > Date.parse(range.from);
  const minCapacity = /^\d+$/.test(capacity.trim()) ? Number(capacity.trim()) : null;
  const lists = useRoomLists(open ? accountId : null);
  const rooms = useRooms(
    open ? accountId : null,
    list || null,
    range.from,
    range.to,
    minCapacity,
    valid,
  );
  const booked = attendees.filter(isRoom);
  const bookedSet = new Set(booked.map((a) => a.email.toLowerCase()));
  const shown = (rooms.data ?? []).filter((r) => !onlyFree || roomState(r) === "free");

  const add = (r: RoomAvailability) => {
    onAttendeesChange(addRoom(attendees, r.room));
    if (location.trim() === "") onLocationChange(r.room.name);
  };
  const remove = (address: string, name: string | null) => {
    onAttendeesChange(removeRoom(attendees, address));
    if (name !== null && location.trim() === name) onLocationChange("");
  };

  return (
    <fieldset data-part="exchange.rooms" data-state={open ? "open" : undefined}>
      <legend data-part="form.section.title">{t("exchange-rooms-title")}</legend>
      {booked.length > 0 && (
        <ul data-part="exchange.rooms.booked" aria-label={t("exchange-rooms-booked")}>
          {booked.map((a) => (
            <li key={a.email} data-part="exchange.rooms.booked.item" data-status={a.status}>
              <span data-part="exchange.room.name">{a.name ?? a.email}</span>
              <span data-part="exchange.room.response">
                {t(`exchange-room-response-${a.status}`)}
              </span>
              <button
                type="button"
                data-part="form.button"
                data-action="remove-room"
                aria-label={t("exchange-room-remove-named", { name: a.name ?? a.email })}
                onClick={() => {
                  remove(a.email, a.name);
                }}
              >
                {t("exchange-room-remove")}
              </button>
            </li>
          ))}
        </ul>
      )}
      <button
        type="button"
        data-part="form.button"
        data-action="find-room"
        aria-expanded={open}
        aria-controls={open ? ids.panel : undefined}
        onClick={() => {
          setOpen((o) => !o);
        }}
      >
        {t(open ? "exchange-rooms-hide" : "exchange-rooms-find")}
      </button>
      {open && (
        <div id={ids.panel} data-part="exchange.rooms.panel">
          <div data-part="exchange.rooms.filters">
            <div data-part="form.field" data-field="room-list">
              <label data-part="form.label" htmlFor={ids.list}>
                {t("exchange-rooms-list")}
              </label>
              <select
                id={ids.list}
                data-part="form.select"
                value={list}
                onChange={(e) => {
                  setList(e.target.value);
                }}
              >
                <option value="">{t("exchange-rooms-all")}</option>
                {(lists.data ?? []).map((l) => (
                  <option key={l.address} value={l.address}>
                    {l.name}
                  </option>
                ))}
              </select>
            </div>
            <div data-part="form.field" data-field="capacity">
              <label data-part="form.label" htmlFor={ids.capacity}>
                {t("exchange-rooms-capacity")}
              </label>
              <input
                id={ids.capacity}
                data-part="form.input"
                data-width="short"
                type="number"
                inputMode="numeric"
                min={1}
                value={capacity}
                onChange={(e) => {
                  setCapacity(e.target.value);
                }}
              />
            </div>
            <label data-part="form.check" data-field="only-free">
              <input
                data-part="form.checkbox"
                type="checkbox"
                checked={onlyFree}
                onChange={(e) => {
                  setOnlyFree(e.target.checked);
                }}
              />
              <span data-part="form.check.label">{t("exchange-rooms-only-free")}</span>
            </label>
          </div>
          {lists.isError && <Problem message={errorMessageOf(lists.error)} />}
          {!lists.isError && rooms.isError && <Problem message={errorMessageOf(rooms.error)} />}
          {!valid && <Problem message={{ id: "exchange-rooms-invalid-time" }} />}
          {(rooms.isFetching || lists.isFetching) && (
            <p data-part="exchange.hint" role="status">
              {t("exchange-rooms-searching")}
            </p>
          )}
          {rooms.data && shown.length === 0 && (
            <p data-part="exchange.rooms.empty">{t("exchange-rooms-none")}</p>
          )}
          {shown.length > 0 && (
            <ul data-part="exchange.rooms.items" aria-label={t("exchange-rooms-results")}>
              {shown.map((r) => (
                <RoomRow
                  key={r.room.address}
                  availability={r}
                  booked={bookedSet.has(r.room.address.toLowerCase())}
                  tz={tz}
                  onAdd={() => {
                    add(r);
                  }}
                  onRemove={() => {
                    remove(r.room.address, r.room.name);
                  }}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </fieldset>
  );
}
