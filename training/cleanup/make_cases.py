"""Writes cases.json and vocab.json. Edit the lists here, then re-run."""

import json
from pathlib import Path

HERE = Path(__file__).parent

VOCAB = ["Rivaan", "Meher Kapadia", "Zubin", "Tavish", "Ishita Venkatraman", "Oshin Dastur",
         "Quorvia", "Brightlane", "Velocore", "Kubernetes", "PostgreSQL", "Next.js", "GraphQL",
         "Supabase", "TypeScript", "Redis"]

# (id, category, app, spoken, expected)
CASES = [
    # fillers and false starts
    ("fil01", "filler", "Slack", "um so I was thinking we could, uh, maybe start the review a bit earlier tomorrow",
     "So I was thinking we could maybe start the review a bit earlier tomorrow."),
    ("fil02", "filler", "Slack", "you know the dashboard is like really slow when you open the filters",
     "The dashboard is really slow when you open the filters."),
    ("fil03", "filler", "Notes", "hmm okay so basically the client wants the logo, uh, bigger on the home page",
     "Okay, so basically the client wants the logo bigger on the home page."),
    ("fil04", "filler", "Slack", "uh hey Rivaan, um, just checking if you got a chance to look at the, uh, pricing sheet",
     "Hey Rivaan, just checking if you got a chance to look at the pricing sheet."),
    ("fil05", "filler", "Messages", "I I think the the numbers look fine to me, honestly",
     "I think the numbers look fine to me, honestly."),
    ("fil06", "filler", "Mail", "so, uh, the thing is, um, we need the signed copy before Friday",
     "So the thing is, we need the signed copy before Friday."),
    # self-corrections
    ("cor01", "correction", "Slack", "hey can we, um, move the call to four, no, five",
     "Hey, can we move the call to 5?"),
    ("cor02", "correction", "Slack", "send the report to Zubin, sorry, I meant Tavish",
     "Send the report to Tavish."),
    ("cor03", "correction", "Mail", "the meeting is on Tuesday, actually no, Wednesday at ten",
     "The meeting is on Wednesday at 10."),
    ("cor04", "correction", "Slack", "let's order twenty units, scratch that, let's order fifty units",
     "Let's order 50 units."),
    ("cor05", "correction", "Messages", "I'll be there by six, I mean seven, if the traffic is okay",
     "I'll be there by 7, if the traffic is okay."),
    ("cor06", "correction", "Slack", "can you ping Meher, no wait, ping Ishita about the contract",
     "Can you ping Ishita about the contract?"),
    ("cor07", "correction", "Mail", "the budget is ten lakh, no sorry, twelve lakh for this quarter",
     "The budget is 12 lakh for this quarter."),
    ("cor08", "correction", "Slack", "we should, we should probably push the release to Friday",
     "We should probably push the release to Friday."),
    # spoken punctuation
    ("pun01", "spoken_punct", "Slack", "thanks for the update comma I'll take a look tomorrow period",
     "Thanks for the update, I'll take a look tomorrow."),
    ("pun02", "spoken_punct", "Mail", "hi Meher new line please find the revised quote attached period new line thanks",
     "Hi Meher,\nPlease find the revised quote attached.\nThanks"),
    ("pun03", "spoken_punct", "Slack", "is the build green question mark",
     "Is the build green?"),
    ("pun04", "spoken_punct", "Messages", "that's amazing exclamation mark congrats to the whole team",
     "That's amazing! Congrats to the whole team."),
    ("pun05", "spoken_punct", "Notes", "the options are red comma blue comma or green period",
     "The options are red, blue, or green."),
    # numbers, times, money, dates
    ("num01", "numbers", "Mail", "the invoice comes to two thousand four hundred and fifty dollars",
     "The invoice comes to $2,450."),
    ("num02", "numbers", "Slack", "let's meet at three thirty PM on Thursday",
     "Let's meet at 3:30 PM on Thursday."),
    ("num03", "numbers", "Slack", "conversion went up by twelve percent last week",
     "Conversion went up by 12% last week."),
    ("num04", "numbers", "Mail", "the launch is on March fourteenth twenty twenty seven",
     "The launch is on March 14, 2027."),
    ("num05", "numbers", "Messages", "we need about fifteen hundred rupees for the cab tonight",
     "We need about ₹1,500 for the cab tonight."),
    ("num06", "numbers", "Messages", "call me at nine eight seven six five four three two one zero",
     "Call me at 9876543210."),
    ("num07", "numbers", "Slack", "we shipped version two point four point one yesterday",
     "We shipped version 2.4.1 yesterday."),
    # emails and URLs
    ("eml01", "email_url", "Slack", "send it to meher dot kapadia at velocore dot io",
     "Send it to meher.kapadia@velocore.io."),
    ("eml02", "email_url", "Slack", "the docs are at docs dot brightlane dot dev slash setup",
     "The docs are at docs.brightlane.dev/setup."),
    ("eml03", "email_url", "Mail", "my email is rivaan underscore m at quorvia dot com",
     "My email is rivaan_m@quorvia.com."),
    ("eml04", "email_url", "Messages", "go to quorvia dot com slash pricing and check the annual plan",
     "Go to quorvia.com/pricing and check the annual plan."),
    ("eml05", "email_url", "Mail", "please CC tavish at brightlane dot dev on the thread",
     "Please CC tavish@brightlane.dev on the thread."),
    # lists said aloud
    ("lst01", "list", "Notes", "three things for today, number one, finish the deck, number two, call the vendor, number three, send the invoice",
     "Three things for today:\n1. Finish the deck\n2. Call the vendor\n3. Send the invoice"),
    ("lst02", "list", "Notes", "agenda, bullet, budget review, bullet, hiring plan, bullet, Q4 roadmap",
     "Agenda:\n- Budget review\n- Hiring plan\n- Q4 roadmap"),
    ("lst03", "list", "Claude Code", "next steps are one, ping Zubin about the API keys, two, update the Kubernetes config, and three, rerun the tests",
     "Next steps are:\n1. Ping Zubin about the API keys\n2. Update the Kubernetes config\n3. Rerun the tests"),
    ("lst04", "list", "Slack", "for the offsite we need, first, a venue, second, a budget, and third, a date everyone can make",
     "For the offsite we need:\n1. A venue\n2. A budget\n3. A date everyone can make"),
    # names from the vocabulary list
    ("voc01", "vocab", "Slack", "Ishita Venkatraman will join the call from the Velocore side",
     "Ishita Venkatraman will join the call from the Velocore side."),
    ("voc02", "vocab", "Slack", "can you add Tavish and Rivaan to the Quorvia channel",
     "Can you add Tavish and Rivaan to the Quorvia channel?"),
    ("voc03", "vocab", "Mail", "Brightlane signed the renewal, please let Meher Kapadia know today",
     "Brightlane signed the renewal, please let Meher Kapadia know today."),
    ("voc04", "vocab", "Messages", "Zubin said the Quorvia demo went really well yesterday",
     "Zubin said the Quorvia demo went really well yesterday."),
    ("voc05", "vocab", "Mail", "please loop in Oshin Dastur from legal before we sign anything",
     "Please loop in Oshin Dastur from legal before we sign anything."),
    # tech terms
    ("tec01", "tech", "Slack", "we should move the sessions table from Redis to Postgres Q L",
     "We should move the sessions table from Redis to PostgreSQL."),
    ("tec02", "tech", "Claude Code", "the Next J S app is calling the A P I twice on every render",
     "The Next.js app is calling the API twice on every render."),
    ("tec03", "tech", "Slack", "the Kubernetes pods keep restarting after the deploy, um, can you check",
     "The Kubernetes pods keep restarting after the deploy, can you check?"),
    ("tec04", "tech", "Claude Code", "add a graph Q L endpoint for the orders A P I and write tests for it",
     "Add a GraphQL endpoint for the orders API and write tests for it."),
    ("tec05", "tech", "Claude Code", "the type script build fails because of a missing env variable in Supabase",
     "The TypeScript build fails because of a missing env variable in Supabase."),
    # questions
    ("que01", "question", "Slack", "hey do you know if the, uh, the contract got signed",
     "Hey, do you know if the contract got signed?"),
    ("que02", "question", "Slack", "what time is the standup tomorrow",
     "What time is the standup tomorrow?"),
    ("que03", "question", "Claude Code", "can you explain why the tests are failing on the main branch",
     "Can you explain why the tests are failing on the main branch?"),
    ("que04", "question", "Claude Code", "um so how do I reverse a linked list in Python without using extra memory",
     "So how do I reverse a linked list in Python without using extra memory?"),
    # long run-ons
    ("run01", "run_on", "Slack",
     "so quick update on the migration we moved about half the tables last night and um the rest should be done by Thursday "
     "but the reporting jobs are going to be a bit slow until then so if anyone sees timeouts just let me know",
     "So, quick update on the migration: we moved about half the tables last night and the rest should be done by Thursday, "
     "but the reporting jobs are going to be a bit slow until then. So if anyone sees timeouts, just let me know."),
    ("run02", "run_on", "Mail",
     "hi Oshin thanks for sending the draft I went through it and I think section two needs a bit more detail on the payment terms "
     "and also uh the dates in the appendix don't match the main contract can you take a look",
     "Hi Oshin, thanks for sending the draft. I went through it and I think section 2 needs a bit more detail on the payment terms, "
     "and also the dates in the appendix don't match the main contract. Can you take a look?"),
    ("run03", "run_on", "Claude Code",
     "okay so I want you to refactor the auth module so that it uses the new token service and um make sure the old tests still pass "
     "and then also add a test for the expired token case",
     "Okay, so I want you to refactor the auth module so that it uses the new token service and make sure the old tests still pass, "
     "and then also add a test for the expired token case."),
    ("run04", "run_on", "Messages",
     "hey I'm running a bit late the metro is super crowded today I'll probably reach by eight fifteen so you guys go ahead and order",
     "Hey, I'm running a bit late. The metro is super crowded today. I'll probably reach by 8:15, so you guys go ahead and order."),
    # Hinglish in Roman letters
    ("hng01", "hinglish", "Slack", "um kal ki meeting cancel kar do, Rivaan ko bata dena",
     "Kal ki meeting cancel kar do, Rivaan ko bata dena."),
    ("hng02", "hinglish", "Slack", "yaar build phir se fail ho gaya, uh, logs check karo",
     "Yaar, build phir se fail ho gaya, logs check karo."),
    ("hng03", "hinglish", "Messages", "main paanch baje aaunga, nahi nahi, chhe baje",
     "Main 6 baje aaunga."),
    ("hng04", "hinglish", "Slack", "Meher ko bolo ki invoice kal tak bhej de, client wait kar raha hai",
     "Meher ko bolo ki invoice kal tak bhej de, client wait kar raha hai."),
    ("hng05", "hinglish", "Slack", "matlab deploy aaj shaam tak ho jayega na",
     "Matlab deploy aaj shaam tak ho jayega na?"),
    ("hng06", "hinglish", "Slack", "client ka email aaya kya, uh, abhi tak, mujhe kuch nahi mila",
     "Client ka email aaya kya abhi tak? Mujhe kuch nahi mila."),
    ("hng07", "hinglish", "Slack", "mujhe lagta hai ki hum, hum log Friday ko launch kar sakte hain",
     "Mujhe lagta hai ki hum log Friday ko launch kar sakte hain."),
    ("hng08", "hinglish", "Messages", "bhai woh PR merge kar de please, tests pass ho gaye hain",
     "Bhai, woh PR merge kar de please, tests pass ho gaye hain."),
    ("hng09", "hinglish", "Slack", "ek kaam karo, Zubin ko call karo aur pucho ki budget final hua ya nahi",
     "Ek kaam karo, Zubin ko call karo aur pucho ki budget final hua ya nahi."),
    ("hng10", "hinglish", "Messages", "aaj raat dinner pe chalein kya, main aath baje free hoon",
     "Aaj raat dinner pe chalein kya? Main 8 baje free hoon."),
]


def main():
    ids = [c[0] for c in CASES]
    assert len(ids) == len(set(ids))
    cases = [{"id": i, "category": cat, "spoken": s, "expected": e, "app": app} for i, cat, app, s, e in CASES]
    (HERE / "cases.json").write_text(json.dumps(cases, indent=1, ensure_ascii=False) + "\n")
    (HERE / "vocab.json").write_text(json.dumps(VOCAB, indent=1) + "\n")
    print(len(cases), "cases")


if __name__ == "__main__":
    main()
