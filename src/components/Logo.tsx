import appIcon from "../../assets/brand/bdm-app-icon.svg";
import mark from "../../assets/brand/bdm-mark.svg";

interface LogoProps {
  size?: number;
  title?: string;
  /** The full app icon (navy tile) instead of the bare mark (for dark backgrounds). */
  tile?: boolean;
}

/** Babarify logo from the Brandboard: glossy blue-to-green "B" with a download arrow (assets/brand). */
export function Logo({ size = 32, title = "Babarify Download Manager", tile = false }: LogoProps) {
  return <img src={tile ? appIcon : mark} width={size} height={size} alt={title} draggable={false} className="logo-img" />;
}

export const TAGLINE = "Faster Downloads. More Possibilities.";
