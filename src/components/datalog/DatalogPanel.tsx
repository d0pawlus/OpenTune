// SPDX-License-Identifier: GPL-3.0-or-later
import { t, type Locale } from "../../i18n";
import { useDatalogStore } from "../../stores/datalog";
import { Analysis } from "./AnalysisSection";
import { ChartControls } from "./ChartControls";
import "./datalog.css";
import { ExportControls, LogPathForm, RecordingControls } from "./LogSlotForms";
import { MarkersSection } from "./MarkersSection";
import { MathChannelLibrary } from "./MathChannelLibrary";
import { Playback } from "./PlaybackControls";

export function DatalogPanel({ locale }: { locale: Locale }) {
  const loading = useDatalogStore((state) => state.loading);
  const error = useDatalogStore((state) => state.error);

  return (
    <section className="datalog" aria-labelledby="datalog-title">
      <header>
        <h2 id="datalog-title">{t("datalog.title", locale)}</h2>
      </header>
      <p className="dl-offline">{t("datalog.offline", locale)}</p>
      {error && (
        <p role="alert" className="dl-error">
          {error}
        </p>
      )}
      {loading && <p role="status">{t("datalog.loading", locale)}</p>}
      <RecordingControls locale={locale} />
      <div className="dl-two-column">
        <LogPathForm slot="A" locale={locale} />
        <LogPathForm slot="B" locale={locale} />
      </div>
      <Playback locale={locale} />
      <MathChannelLibrary locale={locale} />
      <ChartControls locale={locale} />
      <MarkersSection locale={locale} />
      <ExportControls locale={locale} />
      <Analysis locale={locale} />
    </section>
  );
}
