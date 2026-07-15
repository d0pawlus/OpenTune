// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo, useState } from "react";
import { t, type Locale } from "../../i18n";
import { useDatalogStore } from "../../stores/datalog";
import { NumberField } from "./DatalogFormFields";
import type { MathOperation } from "./mathChannels";

export function MathChannelLibrary({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
  const specs = useDatalogStore((state) => state.mathChannels);
  const add = useDatalogStore((state) => state.addMathChannel);
  const remove = useDatalogStore((state) => state.removeMathChannel);
  const channels = useMemo(
    () => (logs.A ?? logs.B)?.summary.fields.map((field) => field.name) ?? [],
    [logs],
  );
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<MathOperation["kind"]>("derivative");
  const [first, setFirst] = useState(5);
  const [second, setSecond] = useState(100);
  const selectedSource = source || channels[0] || "";

  const create = () => {
    if (!selectedSource || !name.trim()) return;
    let operation: MathOperation;
    if (kind === "movingAverage") operation = { kind, window: first };
    else if (kind === "lowPass") operation = { kind, alpha: first };
    else if (kind === "gate") operation = { kind, min: first, max: second };
    else operation = { kind };
    add({
      id: `${Date.now()}-${name.trim()}`,
      name: name.trim(),
      source: selectedSource,
      operation,
    });
    setName("");
  };

  return (
    <fieldset className="dl-fieldset">
      <legend>{t("datalog.math", locale)}</legend>
      <label>
        {t("datalog.source", locale)}
        <select
          value={selectedSource}
          onChange={(event) => setSource(event.target.value)}
        >
          {channels.map((channel) => (
            <option key={channel}>{channel}</option>
          ))}
        </select>
      </label>
      <label>
        {t("datalog.operation", locale)}
        <select
          value={kind}
          onChange={(event) =>
            setKind(event.target.value as MathOperation["kind"])
          }
        >
          <option value="derivative">{t("datalog.derivative", locale)}</option>
          <option value="movingAverage">
            {t("datalog.movingAverage", locale)}
          </option>
          <option value="lowPass">{t("datalog.lowPass", locale)}</option>
          <option value="gate">{t("datalog.gate", locale)}</option>
        </select>
      </label>
      {kind === "movingAverage" && (
        <NumberField
          label={t("datalog.window", locale)}
          value={first}
          min={1}
          step={1}
          onChange={setFirst}
        />
      )}
      {kind === "lowPass" && (
        <NumberField
          label={t("datalog.alpha", locale)}
          value={first}
          min={0}
          max={1}
          step={0.05}
          onChange={setFirst}
        />
      )}
      {kind === "gate" && (
        <>
          <NumberField
            label={t("datalog.minimum", locale)}
            value={first}
            onChange={setFirst}
          />
          <NumberField
            label={t("datalog.maximum", locale)}
            value={second}
            onChange={setSecond}
          />
        </>
      )}
      <label>
        {t("datalog.name", locale)}
        <input value={name} onChange={(event) => setName(event.target.value)} />
      </label>
      <button
        type="button"
        onClick={create}
        disabled={!selectedSource || !name.trim()}
      >
        {t("datalog.create", locale)}
      </button>
      <ul className="dl-inline-list">
        {specs.map((spec) => (
          <li key={spec.id}>
            {spec.name} ← {spec.source}
            <button type="button" onClick={() => remove(spec.id)}>
              {t("datalog.remove", locale)}
            </button>
          </li>
        ))}
      </ul>
    </fieldset>
  );
}
