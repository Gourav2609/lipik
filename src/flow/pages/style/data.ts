export type ContextId = "personal" | "work" | "email" | "other";
export type StyleId = "formal" | "casual" | "very-casual";
export type CleanupLevel = "light" | "medium" | "heavy";

export const CONTEXTS: { id: ContextId; label: string; apps: string }[] = [
  { id: "personal", label: "Messages", apps: "WhatsApp, Telegram, Messages" },
  { id: "work", label: "Team chat", apps: "Slack, Teams, Discord" },
  { id: "email", label: "Email", apps: "Mail, Gmail, Outlook" },
  { id: "other", label: "Everywhere else", apps: "Notes, documents, browsers" },
];

export const STYLES: { id: StyleId; name: string; rule: string }[] = [
  { id: "formal", name: "Formal", rule: "Capitals and full punctuation" },
  { id: "casual", name: "Relaxed", rule: "Capitals, lighter punctuation" },
  { id: "very-casual", name: "Lowercase", rule: "No capitals, little punctuation" },
];

export const SAMPLES: Record<ContextId, Record<StyleId, string>> = {
  personal: {
    formal:
      "Are you around on Saturday? There is a small bookshop near the station that I want to show you.",
    casual:
      "Are you around Saturday? There's a small bookshop near the station I want to show you",
    "very-casual":
      "are you around saturday? there's a small bookshop near the station i want to show you",
  },
  work: {
    formal:
      "The new build is on staging. Please test the sign-up flow before Wednesday so we can release on Friday.",
    casual:
      "New build is on staging. Can you test the sign-up flow before Wednesday so we can release Friday",
    "very-casual":
      "new build is on staging, can you test the sign-up flow before wednesday so we can release friday",
  },
  email: {
    formal:
      "Hi Meera,\n\nThank you for the draft contract. I have two small changes to the payment terms. Are you free for a short call on Monday?\n\nRegards,\nRohan",
    casual:
      "Hi Meera,\n\nThanks for the draft contract. Two small changes on the payment terms, free for a quick call Monday?\n\nRohan",
    "very-casual":
      "hi meera\n\nthanks for the draft contract. two small changes on the payment terms, free for a quick call monday?\n\nrohan",
  },
  other: {
    formal:
      "Buy milk and bread, renew the car insurance, and send the slides to Priya before the review.",
    casual:
      "Buy milk and bread, renew the car insurance and send the slides to Priya before the review",
    "very-casual":
      "buy milk and bread renew the car insurance and send the slides to priya before the review",
  },
};

export const CLEANUP_LEVELS: {
  id: CleanupLevel;
  name: string;
  description: string;
  result: string;
}[] = [
  {
    id: "light",
    name: "Light",
    description: "Takes out filler words. Keeps your wording.",
    result: "So I think we should move the call to Tuesday, actually Wednesday works better.",
  },
  {
    id: "medium",
    name: "Medium",
    description: "Also fixes false starts and repeated words.",
    result: "I think we should move the call to Wednesday.",
  },
  {
    id: "heavy",
    name: "Heavy",
    description: "Rewrites rambling sentences so they read clearly.",
    result: "Let's move the call to Wednesday.",
  },
];

export const CLEANUP_SPOKEN =
  "Um, so I think we should, uh, move the call to Tuesday, actually no, Wednesday works better.";
