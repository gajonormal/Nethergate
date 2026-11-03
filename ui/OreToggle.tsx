import type { InputHTMLAttributes } from "react";
import switchOff from "./assets/ore/switch-off.svg";

/** Native keyboard/form behaviour with the Ore switch's 100 × 56 geometry at half scale. */
export function OreToggle(
  props: Omit<InputHTMLAttributes<HTMLInputElement>, "type">,
) {
  return (
    <span className="ore-toggle">
      <input
        {...props}
        type="checkbox"
        role="switch"
        className="ore-toggle-input"
      />
      <span className="ore-toggle-art" aria-hidden="true">
        <span className="ore-toggle-track">
          <span />
        </span>
        <span className="ore-toggle-on" />
        <img className="ore-toggle-off" src={switchOff} alt="" />
        <span className="ore-toggle-thumb">
          <span />
        </span>
      </span>
    </span>
  );
}
