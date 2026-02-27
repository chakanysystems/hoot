# Hoot — Design Specification

## Philosophy

Hoot should feel rock solid and reliable, but genuinely nice to look at. The analogy is BMW styling with Honda internals — quality that's obvious without announcing itself. Nothing decorative that doesn't earn its place. No visual noise.

The user is in control. They own their identity, their keys, their node. The UI should reinforce this quietly — not with "sovereignty" branding or hacker aesthetics, but through calm confidence and the absence of corporate cruft.

## References

- Mercury Weather (iOS) — serious but warm, nothing wasted
- Signal — confident, quiet, trustworthy
- Apple liquid glass / iOS 26 — depth, material feeling, physical quality
- NOT: Gmail, Outlook, any dark-mode terminal aesthetic

## Theme

**Light.** Warm off-whites, not cool grays. The background should feel like paper, not a screen.

```
--bg:        #f5f4f2   /* page background */
--surface:   #fdfcfa   /* cards, sidebar */
--surface2:  #f0eeeb   /* inputs, chips, hover states */
--border:    rgba(0,0,0,0.07)
--border-strong: rgba(0,0,0,0.12)
```

## Typography

**Font:** Instrument Sans (Google Fonts)
- Body / UI: 13.5px, weight 400
- Sender names, labels: weight 500
- App name: 17px, weight 600, letter-spacing -0.02em
- Always `-webkit-font-smoothing: antialiased`

No monospace in the main UI. Monospace only for npub/key strings and technical identifiers.

## Color

**Accent:** `#7c3aed` (purple)
- Used for: active nav item, compose button, unread dot, badge backgrounds, focus rings
- Used sparingly — one dominant accent, not scattered everywhere

**Text hierarchy:**
```
--text:   #1a1917   /* primary */
--text-2: #6b6762   /* secondary, read messages */
--text-3: #a8a49f   /* timestamps, placeholders, labels */
```

**Status colors:**
```
--green: #16a34a   /* online/verified */
--amber: #d97706   /* warnings */
```

## Components

### Sidebar
- Width: 232px
- Background: `--surface`
- App name at top, no tagline, no logo mark needed yet
- Compose button: full-width, accent color, border-radius 10px, subtle box-shadow
- Nav items: 8px border-radius, left border highlight when active (not background fill alone)
- Account switcher + online dot at the bottom footer — identity lives here, not at the top

### Message List
- Each item: border-radius 10px, hover shows surface + shadow-sm
- Unread indicator: 5px dot, accent color, left edge, vertically centered
- Avatar: 34px, border-radius 9px, soft gradient fills (not photos), initials
- Unread messages: sender weight 500, subject normal weight
- Read messages: sender and subject both `--text-2`
- Timestamps: `--text-3`, right-aligned

### Protocol Badges
Small, understated. Show how a message arrived — don't make it a feature, make it information.

```
nostr   — purple tint bg, purple text
smtp    — neutral bg, --text-2
pgp     — green tint bg, green text
```

Font: 10px, weight 500, border-radius 4px, padding 2px 7px.

### Search
- Background: `--surface2`
- Focus: accent border + `rgba(124,58,237,0.08)` ring
- Icon inside, left-aligned

### Status Bar
Always visible at the bottom of the message list. Shows node health — relay count, latency, nodes seen. Font size 11px, `--text-3` labels, `--text-2` values. This is information, not decoration.

## Interaction

- Hover transitions: 0.1–0.15s, nothing dramatic
- Compose button: subtle translateY(-0.5px) on hover
- No bouncing, no spring animations, no loading skeletons that feel performative
- Scroll bars: 4px, `rgba(0,0,0,0.1)`, no track background

## What to Avoid

- Purple gradients on white — too generic AI
- Dark mode terminal aesthetic — that's not this product
- Overuse of the accent color — one place at a time
- Shadows that feel fake or heavy
- Borders that are too visible — use `rgba` opacity, not solid colors
- Anything that needs to be explained — if it needs a tooltip to justify its existence, cut it
- Default egui / system UI widget styling — every interactive element should be custom styled

---

## Message View

Opens full screen, replacing the message list. Back button top left returns to inbox.

**Layout:**
- Top bar: back arrow, sender name + address, timestamp right-aligned, action icons (reply, archive, delete) far right
- Message header: subject in 17px weight 600, sender info below in --text-2
- Body: 15px, --text, line-height 1.6, max-width ~640px, centered with generous side padding
- If PGP verified or nostr-native: show small badge near sender, not in body
- Attachments: below body, clean list, file icon + name + size, no heavy chrome

**Replies:**
- Reply compose appears below the message body, not a modal
- Thin divider separates original message from reply area
- Send button bottom right of reply area, accent color

---

## Compose Window

Full screen, not a modal. Feels like opening a blank page, not a dialog box.

**Layout:**
- Top bar: close/discard left, "New Message" title center, Send button right (accent, disabled until To and Subject filled)
- Fields: To, Subject — clean underline style inputs, no box borders
- To field: supports npub addresses and traditional email, shows resolved display name if known
- Body: large open textarea, no toolbar by default
- Formatting toolbar: appears on text selection only, minimal (bold, italic, link) — hidden otherwise
- Bottom bar: attach file left, encryption status indicator center, character/size info right

**Encryption indicator:**
- Shows whether message will go nostr-native or smtp bridge
- Green lock = nostr end-to-end
- Gray lock = smtp bridge, not encrypted
- Never alarming, just informative

---

## Onboarding

Hand-holding but not patronizing. One thing per screen. Progress dots at bottom.

**Screen 1 — Welcome**
- Hoot wordmark, one sentence: "Email that belongs to you."
- Two options: "Create new identity" / "I have a key"
- No feature list, no marketing copy

**Screen 2a — Key Generation**
- "We're generating your keys." 
- Show a simple animation (subtle, not performative)
- Once done: show npub (your address, shareable) and nsec (your private key, never share)
- nsec shown with explicit warning — warm amber background, plain language: "This is your private key. If you lose it, we can't recover your account. Write it down somewhere safe."
- "I've saved my key" checkbox must be checked to continue — not a fake checkbox, it should feel like a real moment

**Screen 2b — Import Key**
- Single input: paste nsec or use a keyfile
- Validate inline, show npub preview once valid
- No jargon beyond what's necessary

**Screen 3 — Add a Relay**
- "Relays are servers that deliver your messages. Add at least one."
- Pre-populated suggestion (wss://relay.damus.io or similar)
- Add more option, remove option
- Skip for now available but discouraged — show small note: "Without a relay you won't receive messages"

**Screen 4 — Optional: SMTP Bridge**
- "Want to send and receive from regular email addresses?"
- Simple toggle, if on: fields for SMTP/IMAP credentials
- Can skip, can configure later in settings
- Plain language explanation of what this does

**Screen 5 — Ready**
- Show their npub address
- "You're set up. Your inbox is waiting."
- Single button: "Open Hoot"

---

## Message Requests

When a user you've never interacted with messages you, it lands in Requests, not Inbox.

**Request card (appears when viewing the message):**
- Sits above the message content
- Sender npub, any resolvable display name/picture
- Two buttons: "Allow" (accent) and "Block" (neutral, not red — red is alarming, this is just a decision)
- Allow moves all messages from this sender to inbox going forward
- Block should have one confirmation step — not a modal, just the button changing to "Confirm block" for 3 seconds
- After decision, card disappears and message reads normally

---

## Settings

Sidebar navigation within settings. Back to inbox top left.

**Sections:**

**Identity**
- Display name, profile picture (resolves from nostr if available)
- Your npub (copyable, shown in full)
- Your nsec — hidden by default, reveal requires confirmation, shown with same amber warning as onboarding
- Export key option

**Relays**
- List of connected relays, latency shown next to each
- Online/offline dot per relay
- Add relay: input field inline, not a modal
- Remove: trash icon, no confirmation needed (easy to re-add)
- Relay health: simple — green/amber/red dot, no graphs

**SMTP Bridge**
- Enable/disable toggle at top
- SMTP host, port, username, password
- IMAP host, port
- Test connection button — shows result inline, not a toast
- Plain language note explaining what this does and the tradeoff (messages via bridge are not end-to-end encrypted)

**Spam & Filters**
- Sensitivity slider for spam filter (low / medium / high)
- Report spam contributes to local model training — one line note
- Block list: table of blocked addresses, remove option

**Notifications**
- Simple toggles, nothing complex

**About**
- Version, licenses, source link

**Settings tone:** Every setting should have a one-line description in --text-3 below it. Plain language. No tooltips needed if the description is good.

---

## Empty States

Every empty state should say what happened and what to do, in plain language. No illustrations unless they genuinely help. No "Wow, so empty!" copy.

- Empty inbox: "No messages yet. Your address is **npub1q8x…f7a2**"
- Empty drafts: "No drafts."
- Empty search: "No messages matching [query]."
- Offline: "Can't reach your relays. Check your connection." — amber, not red

---

## Voice

Tone for all UI copy — empty states, labels, confirmations, warnings:
- Direct, not cute
- Informative, not apologetic  
- Short sentences
- No exclamation marks
- No "Oops" or "Uh oh" for errors — just say what happened
- Treat the user as technically capable even during onboarding
