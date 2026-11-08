import { useState } from "react";
import { PosLayout } from "@/app/layouts/PosLayout";
import { TopBar } from "@/app/layouts/TopBar";
import { useApplyTheme, type Theme } from "@/components/common/ThemeToggle";

/** Application shell. The first-run wizard, login and till screens are routed here from Block 0.9. */
function App() {
  const [theme, setTheme] = useState<Theme>("light");
  useApplyTheme(theme);
  return (
    <PosLayout topBar={<TopBar storeName={null} deviceCode={null} theme={theme} onThemeChange={setTheme} />}>
      <div className="flex-1" />
    </PosLayout>
  );
}

export default App;
