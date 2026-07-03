// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Field } from "./Field";
import type { ConstantDto } from "../../ipc/bindings";

const scalar: ConstantDto = {
  name: "reqFuel",
  units: "ms",
  digits: 1,
  low: 0,
  high: 6553.5,
  kind: "Scalar",
};

const bits: ConstantDto = {
  name: "injLayout",
  units: "",
  digits: 0,
  low: null,
  high: null,
  kind: {
    Bits: { options: ["Paired", "Semi-Sequential", "Banked", "Sequential"] },
  },
};

describe("Field", () => {
  it("renders a scalar as a number input bound to value, units and limits", () => {
    render(
      <Field constant={scalar} value={{ Scalar: 12.5 }} onChange={() => {}} />,
    );
    const input = screen.getByLabelText("reqFuel") as HTMLInputElement;
    expect(input.type).toBe("number");
    expect(input.value).toBe("12.5");
    expect(input.min).toBe("0");
    expect(input.max).toBe("6553.5");
    expect(input.step).toBe("0.1"); // digits = 1
    expect(screen.getByText("ms")).toBeTruthy();
  });

  it("emits a Scalar value on scalar edit", () => {
    const onChange = vi.fn();
    render(
      <Field constant={scalar} value={{ Scalar: 12.5 }} onChange={onChange} />,
    );
    fireEvent.change(screen.getByLabelText("reqFuel"), {
      target: { value: "20" },
    });
    expect(onChange).toHaveBeenCalledWith({ Scalar: 20 });
  });

  it("renders a bits constant as a select with its named options", () => {
    render(<Field constant={bits} value={{ Enum: 2 }} onChange={() => {}} />);
    const select = screen.getByLabelText("injLayout") as HTMLSelectElement;
    expect(select.value).toBe("2");
    const options = Array.from(select.options).map((o) => o.textContent);
    expect(options).toEqual([
      "Paired",
      "Semi-Sequential",
      "Banked",
      "Sequential",
    ]);
  });

  it("emits an Enum value on select change", () => {
    const onChange = vi.fn();
    render(<Field constant={bits} value={{ Enum: 0 }} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("injLayout"), {
      target: { value: "3" },
    });
    expect(onChange).toHaveBeenCalledWith({ Enum: 3 });
  });

  it("disables the control when disabled", () => {
    render(
      <Field
        constant={scalar}
        value={{ Scalar: 1 }}
        disabled
        onChange={() => {}}
      />,
    );
    expect(
      (screen.getByLabelText("reqFuel") as HTMLInputElement).disabled,
    ).toBe(true);
  });
});
