import type { Metadata } from "next";
import { DocsMobileDrawer } from "@/components/docs/DocsMobileDrawer";
import { DocsSidebar } from "@/components/docs/DocsSidebar";
import { Footer } from "@/components/sections/Footer";
import { Navbar } from "@/components/sections/Navbar";

export const metadata: Metadata = {
  title: { template: "%s | ContextOS Docs", default: "Documentation" },
  description:
    "How ContextOS reduces tokens for AI coding assistants — install guide, pipeline internals, and algorithms.",
};

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <>
      <Navbar />
      <div className="container-tight grid grid-cols-1 gap-10 py-10 lg:grid-cols-[240px_1fr] lg:gap-16 lg:py-16">
        <aside className="hidden lg:block">
          <div className="sticky top-24">
            <DocsSidebar />
          </div>
        </aside>
        <div>
          <div className="mb-6 lg:hidden">
            <DocsMobileDrawer />
          </div>
          <main className="pb-24">{children}</main>
        </div>
      </div>
      <Footer />
    </>
  );
}
