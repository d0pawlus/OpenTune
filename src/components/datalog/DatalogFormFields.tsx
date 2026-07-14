// SPDX-License-Identifier: GPL-3.0-or-later
export const NumberField = ({
  label,
  value,
  onChange,
  min,
  max,
  step = "any",
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number | "any";
}) => (
  <label>
    {label}
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(event) => onChange(Number(event.target.value))}
    />
  </label>
);

export const ChannelSelect = ({
  label,
  value,
  channels,
  setValue,
}: {
  label: string;
  value: string;
  channels: string[];
  setValue: (value: string) => void;
}) => (
  <label>
    {label}
    <select value={value} onChange={(event) => setValue(event.target.value)}>
      {channels.map((channel) => (
        <option key={channel}>{channel}</option>
      ))}
    </select>
  </label>
);
