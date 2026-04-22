import { CodeShowcase } from "@/components/sections/CodeShowcase";
import { FeatureGrid } from "@/components/sections/FeatureGrid";
import { Footer } from "@/components/sections/Footer";
import { Hero } from "@/components/sections/Hero";
import { InstallSection } from "@/components/sections/InstallSection";
import { Navbar } from "@/components/sections/Navbar";
import { Pipeline } from "@/components/sections/Pipeline";

export default function Home() {
  return (
    <>
      <Navbar />
      <main>
        <Hero />
        <Pipeline />
        <FeatureGrid />
        <CodeShowcase />
        <InstallSection />
      </main>
      <Footer />
    </>
  );
}
