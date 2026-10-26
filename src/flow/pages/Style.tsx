import { PageHeader } from "../ui";
import { AutoCleanup } from "./style/AutoCleanup";
import { ToneList } from "./style/ToneList";

export default function Style() {
  return (
    <div className="fk-page">
      <PageHeader
        title="Tone"
        description="How your words are cleaned up in each app."
      />
      <ToneList />
      <div className="mt-[40px]">
        <AutoCleanup />
      </div>
    </div>
  );
}
