// SPDX-License-Identifier: GPL-3.0-or-later
import { t, type Locale } from "../../i18n";
import { useDatalogStore } from "../../stores/datalog";

export function MarkersSection({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
  const markers = [
    ...(logs.A?.markers.map((marker) => ({ slot: "A", marker })) ?? []),
    ...(logs.B?.markers.map((marker) => ({ slot: "B", marker })) ?? []),
  ];

  return (
    <section>
      <h3>{t("datalog.markers", locale)}</h3>
      {markers.length === 0 ? (
        <p>{t("datalog.noMarkers", locale)}</p>
      ) : (
        <ol>
          {markers.map(({ slot, marker }) => (
            <li key={`${slot}-${marker.record_index}-${marker.text}`}>
              {slot} · #{marker.record_index} · {marker.text}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
