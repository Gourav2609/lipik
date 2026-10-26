import { useEffect, useRef, useState, type ReactNode } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { isFlowPreview } from "../dev/preview";
import { Button } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { FIELD_CLASS, SettingRow, SettingsCard } from "./parts";
import { useProfile, type Profile } from "./profile";
import { APP_NAME } from "@/flow/brand";

const Row = ({ label, children }: { label: string; children: ReactNode }) => (
  <SettingRow title={label} control={children} className="py-[12px]" />
);

const confirmDelete = (message: string) =>
  isFlowPreview
    ? Promise.resolve(window.confirm(message))
    : ask(message, { title: "Delete profile", kind: "warning" });

const Field = (props: {
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
}) => (
  <input
    aria-label={props.placeholder}
    value={props.value}
    placeholder={props.placeholder}
    onChange={(e) => props.onChange(e.target.value)}
    className={`${FIELD_CLASS} w-[224px] px-[12px]`}
  />
);

/** Scale the chosen image down to a 128 px square so it fits in storage. */
const toAvatar = (file: File) => {
  const url = URL.createObjectURL(file);
  return new Promise<string>((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const size = 128;
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = size;
      const ctx = canvas.getContext("2d");
      if (!ctx) return reject(new Error("no canvas"));
      const s = Math.min(img.width, img.height);
      ctx.drawImage(
        img,
        (img.width - s) / 2,
        (img.height - s) / 2,
        s,
        s,
        0,
        0,
        size,
        size,
      );
      resolve(canvas.toDataURL("image/jpeg", 0.85));
    };
    img.onerror = reject;
    img.src = url;
  }).finally(() => URL.revokeObjectURL(url));
};

export default function Account() {
  const { profile, save, clear } = useProfile();
  const [draft, setDraft] = useState<Profile>(profile);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => setDraft(profile), [profile]);

  const dirty =
    draft.firstName !== profile.firstName ||
    draft.lastName !== profile.lastName ||
    draft.picture !== profile.picture;
  const set = (patch: Partial<Profile>) => setDraft({ ...draft, ...patch });
  const initial = (draft.firstName || draft.email || "?")
    .charAt(0)
    .toUpperCase();

  return (
    <SettingsPage
      title="Profile"
      intro="Your name is used in greetings. It stays on this Mac."
    >
      <div>
        <SettingsCard>
          <Row label="First name">
            <Field
              value={draft.firstName}
              placeholder="First name"
              onChange={(firstName) => set({ firstName })}
            />
          </Row>
          <Row label="Last name">
            <Field
              value={draft.lastName}
              placeholder="Last name"
              onChange={(lastName) => set({ lastName })}
            />
          </Row>
          <Row label="Email">
            <span className="text-[14.5px] leading-[22px] text-[var(--fk-text-muted)]">
              {profile.email || "Not set"}
            </span>
          </Row>
          <Row label="Picture">
            <button
              type="button"
              aria-label="Change profile picture"
              onClick={() => fileRef.current?.click()}
              className="flex h-[40px] w-[40px] items-center justify-center rounded-full"
            >
              {draft.picture ? (
                <img
                  src={draft.picture}
                  alt=""
                  className="h-[40px] w-[40px] rounded-full object-cover"
                />
              ) : (
                <span className="fk-display flex h-[40px] w-[40px] items-center justify-center rounded-full bg-[var(--fk-accent-soft)] text-[18px] text-[var(--fk-accent)]">
                  {initial}
                </span>
              )}
            </button>
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0];
                e.target.value = "";
                if (file)
                  toAvatar(file)
                    .then((picture) => set({ picture }))
                    .catch(() => {});
              }}
            />
          </Row>
        </SettingsCard>
        <div className="mt-[24px] flex items-center">
          <Button
            variant="danger"
            onClick={async () => {
              if (
                await confirmDelete(
                  `Delete your ${APP_NAME} profile? This can't be undone.`,
                )
              )
                clear();
            }}
          >
            Delete profile
          </Button>
          <Button
            className="ml-auto"
            disabled={!dirty}
            onClick={() => save(draft)}
          >
            Save
          </Button>
        </div>
      </div>
    </SettingsPage>
  );
}
