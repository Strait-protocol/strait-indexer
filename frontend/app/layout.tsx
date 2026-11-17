import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  // Absolute base for Open Graph / Twitter card URLs, so shared links resolve to
  // the canonical domain rather than a *.vercel.app deployment URL.
  metadataBase: new URL("https://straitprotocol.com"),
  title: "Strait — Real-Time Tunnel Indexer for Hemi",
  description:
    "Strait indexes every asset moving through Hemi's Bitcoin and Ethereum tunnels. Real-time, reorg-safe, unified GraphQL API and webhooks.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col">{children}</body>
    </html>
  );
}
