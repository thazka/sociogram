import type { ReactNode } from "react";
import "./globals.css";

export const metadata = {
  title: "sociogram + Next.js",
  icons: { icon: "data:," },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
