# WeChat and QQ message channels

Message channels deliver text and attachments from WeChat or QQ to the nl2sh Agent on the current device. Enabled diagnostic tools, answers and approval requests use the originating conversation. [Configure a model](../getting-started/configure-provider.md), then run the TUI, foreground Web UI or managed `service`. Channels run with these entry points' Web service; one-shot CLI and standalone MCP/A2A servers do not start them.

## Management

Choose Channels in the Web left menu. The page refreshes every two seconds and supports binding, starting, stopping and unbinding. In the TUI, enter `/channels`: Up/Down selects actions or QQ fields, typing edits fields, Enter applies, Ctrl+D unbinds the selected channel, PageUp/PageDown scrolls, and Esc closes. Changes take effect immediately. Closing settings leaves the channel running; stopping cancels its current task and pending approvals.

States distinguish stopped, connecting, awaiting scan, scanned, online, expired and error. Received/replied counters cover this process; pending includes information requests. Online confirms a protocol connection, without guaranteeing model requests or individual replies succeed.

## WeChat iLink

1. Choose Get binding QR URL. Both TUI and Web display a temporary URL; open it to view the QR code.
2. Scan with WeChat and confirm there. The interface shows scanned while waiting, then saves credentials and starts long polling.
3. Send text to the paired bot/contact, for example “Inspect device memory and storage, explain free space, and make no changes.”

iLink connects personal WeChat bots, independently of WeCom and public accounts. Request a new QR when it expires; pair again when credentials expire. Rebinding stops the old connection; unbinding deletes credentials. Replies echo the contact's current `context_token`; the adapter does not broadcast unsolicited messages.

## Official QQ Bot

Create a bot at the [QQ Bot platform](https://q.qq.com/), obtain AppID and AppSecret, enable relevant direct/group messaging permissions and configure the platform's availability requirements. Enter AppID, AppSecret and the bot QQ number in Web Channels or TUI `/channels`, then choose Save and connect. A blank secret preserves existing credentials; changing AppID requires a new secret.

Find the bot QQ number in the platform. It completes the official add-bot / group invitation URL. After saving, Web displays a locally generated QR code and copyable link; TUI displays the QR and URL. Scan with mobile QQ to open the bot profile and add it or invite it to a group as platform permissions allow. This pairs the official bot; it does not log in a personal QQ account.

Supports `C2C_MESSAGE_CREATE` direct messages and `GROUP_AT_MESSAGE_CREATE` group mentions. Mention the bot when sending group commands too. The adapter uses the official outbound WebSocket gateway and API v2 passive replies, requiring no public webhook, OneBot service or personal QQ login. The current official API base is `https://api.bot.qq.com`.

## Conversations, approvals and questions

Send natural-language diagnostic requests. Each combination of channel, bot account, conversation and sender has independent model history, including individual group members. Each channel processes diagnostic tasks serially with a queue of up to 32 messages. Intake and approval handling stay separate, so waiting for a decision does not block protocol heartbeats.

| Message | Purpose |
| --- | --- |
| `/new` | Start fresh model history, retaining saved snapshots |
| `/cancel` | Request cancellation of your current task |
| `/approve` | Approve the single pending operation once; clear it and ask again next time |
| `/approve session` | Allow all ordinary operations for this sender/conversation until `/new` or restart; high risk / Root still prompt |
| `/approve always` | Persist its pattern across channel conversations and restarts |
| `/approve REQUEST_ID` | Approve a specified request once when multiple requests are pending |
| `/approve REQUEST_ID CONFIRM REQUEST_ID` | Enter the complete dangerous-operation phrase shown in the prompt |
| `/deny` | Refuse the single current pending request |
| `/deny REQUEST_ID` | Refuse a specified request when multiple requests are pending |
| `/answer REQUEST_ID content` | Answer the current information request; use the displayed option value or custom text |

Approval messages include the full tool/operation preview, risk, Root marker and matched rules. Operations whose previews cannot fit are refused instead of requesting approval of a truncated preview. Dangerous operations first require the ordinary decision, then a second one-shot request with the full strong phrase. Requests expire after 120 seconds and accept a decision only from the originating sender in the same channel, account and conversation. Wrong IDs, cross-conversation responses, missing strong phrases and repeated decisions cannot approve execution. Read-only operations follow the existing confirmation policy; configured scoped approval grants retain their existing matching rules.

Channel decisions are independent of `protocol_auto_approve` and TUI `/permission allow`. MCP/A2A retain their existing local approval policy; channel commands cannot approve protocol tasks. One-shot decisions apply to the current operation; pattern grants match only after security classification and preserve parameter and target checks.


`/approve session` releases the current request and grants all ordinary operations for its originating sender, channel account and conversation in memory. Different subsequent commands, screenshots and file deliveries need no additional approval. Each operation is reassessed and targets revalidated; Dangerous, Critical, Root and strong-confirmation operations still prompt individually. A current dangerous request must complete its second confirmation before establishing the ordinary-operation session grant.

For image, audio, video and file delivery, `/approve always` matches the destination and resolved media kind supplied by trusted preparation, bound to the original requester/conversation and retaining tool, risk and rule checks. Changes to file path, name, contents, size, digest or caption do not prompt again. The approval shows the permanent scope explicitly; every delivery still captures immutable bytes. Other operations retain exact complete-preview matching without program-name wildcards. Legacy permanent records keep their exact scope; approving with `/approve always` again saves the new scope. Dangerous patterns require initial strong confirmation; Root / Critical cannot be permanently authorized. Refusal or expiration creates no grant, and the same refused action cannot reprompt through a tool alias within that task; a new user request can ask again. Bare commands select one pending request in the original route; multiple pending requests require IDs.

Session grants remain in memory, expire on `/new` or restart, and support at most 512 routes. Permanent `command_allowlist` entries are stored privately in `channels/accounts.json`. Media grants bind the original route and destination; other exact patterns apply within this configuration without changing TUI/Web/MCP approvals. Revoke an entry by its approval ID through owner management, or stop the owner, remove the entry and restart; unbinding does not clear permanent entries. The permanent list holds at most 256 patterns and 512 KiB.

## Images, audio and files

Both channels receive and send images, audio, video and other files, including attachment-only messages. WeChat media is downloaded from iLink CDN and decrypted with AES-128-ECB / PKCS#7; QQ uses official attachment URLs and chunked uploads. Limits: 20 MiB per file, eight attachments and 32 MiB total per message. Failed downloads are reported rather than treated as diagnostic evidence.

Files are staged in private `channels/media/` with names, types and paths supplied to tools. Up to four decodable images become bounded JPEG previews passed directly to a vision-capable model. Audio and other files use appropriate inspection tools; platform speech transcripts are labeled unverified. General speech recognition and automatic codec conversion are not provided. WeChat sends original audio as a file; QQ sends SILK, MP3, WAV and OGG as voice and other codecs as files.

Ask “Send me this report / image / audio file.” The Agent uses `send_channel_file` or `send_channel_message`; outside channel tasks an explicit discovered target is required. Approval shows the originating destination, file name, type, size and SHA-256. Delivery uses immutable bytes captured before approval, even if the source changes. A path in a text answer does not send a file.

Cache directories are 0700 and files 0600, with a 256 MiB budget. Files older than seven days are removed during subsequent intake; insufficient space rejects new attachments. Saved history retains attachment descriptions without image Base64, media keys or signed URLs. Historical paths may expire. Attachments and platform transcripts are untrusted input and cannot authorize operations; file delivery requires approval in the originating chat.

## Persistence and limits

Credentials, enabled settings, the WeChat cursor and the latest 2048 message IDs live in private `channels/accounts.json` under the configuration state directory (directory 0700, file 0600). An exclusive lock ensures one protocol owner per configuration. Additional TUI/Web processes owned by the same user manage the existing channels through local IPC; closing a secondary interface leaves the background connection running. QR URLs, access tokens and context tokens are not saved as conversation messages. Snapshots use a `channel-` namespace. Restarts restore completed history and permanent grants without restoring pending approvals or session grants, or replaying unfinished tasks. Duplicate IDs within the bounded ledger are not executed again; crashes can leave messages unanswered, requiring a new message to retry.

There is no sender allowlist. Anyone who can message the connected bot can request device diagnosis and approve their own operations. Group requests, operation previews and replies are visible to group members. Keep the bot in trusted conversations; management uses the [existing Web access model](web.md).

QQ supports ordinary approval callback buttons and text/Markdown command buttons. WeChat does not support buttons; neither channel supports streaming edits or guild channels. Input is limited to 8192 UTF-8 bytes; replies are split into bounded protocol messages with explicit truncation. QQ passive-reply quotas, expiration and moderation remain platform constraints. Failed delivery never re-executes diagnostics. Network errors use bounded reconnect backoff; expired credentials require repairing the binding.

References: [Hermes WeChat adapter](https://github.com/NousResearch/hermes-agent/blob/main/gateway/platforms/weixin.py), [Hermes QQ adapter](https://github.com/NousResearch/hermes-agent/tree/main/gateway/platforms/qqbot), QQ's [official gateway/events documentation](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html) and [access-token documentation](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/access-token.html).

QQ media transport follows the [official rich-media documentation](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/rich-media.html) and [part completion API](https://bot.q.qq.com/wiki/develop/api-v2/autogen/api/v2_users_user_id_upload_part_finish.post.html).

Images, audio, video and arbitrary files all use `send_channel_file`, with exactly one of `path` or a tool-returned current-task `attachment_id`, and `kind=auto/image/audio/video/file`. For screenshots, call `android.screenshot` without a path, obtain its attachment ID, then use the generic delivery tool. No directory creation or persistent screenshot is required; only delivery prompts for approval. References are task-isolated, bounded to eight attachments and 32 MiB total, and cannot be reused from history, another sender or a later task. Delivery snapshots bytes before approval and never reopens the source afterward.

WeChat iLink sends one item per `sendmessage` request, separating caption and media with distinct client IDs. Partial failures are reported without automatic retries or claiming full success; ordinary WeChat audio is delivered as a file.

## Markdown and QQ interactions

Final diagnostic answers use Markdown. QQ sends native `msg_type=2` with `markdown.content` and omits ordinary `content`. WeChat sends compatible Markdown through its text API: bold, tables, fenced and inline code remain; level-five/six heading markers and CJK italic markers are removed. Markdown images are not uploads; use media messages. Rendering depends on the client.

QQ ordinary approvals include Allow once, Session, Always and Deny callback buttons. Only the original requester may operate them; the server independently checks account, direct/group conversation, member, request ID and expiry. Duplicate clicks cannot execute again. Dangerous operations still require typing the complete second-confirmation phrase. Slash approvals remain available.

The adapter subscribes to `INTERACTION (1 << 26)` and handles `INTERACTION_CREATE`. Button/menu events promptly call `PUT /interactions/{interaction_id}`, once per event, with a two-second intake response budget. This acknowledgement describes interaction handling, not successful command execution. QQ Markdown/buttons require the bot's relevant platform permissions; rejection and partial delivery fail explicitly without automatic replay.

Approval prompts, information-request prompts and approval buttons use only the configured `ui_language`. Chinese is the default when omitted:

```toml
ui_language = "en" # English; Chinese is "zh_cn"
```

Commands, tool names, user content and original rule descriptions remain unchanged.

## Shared Agent, MCP and A2A sending tools

All entry points share these tools and retain their own confirmation policy:

- `list_channel_targets`: read-only discovery of conversations observed by the running gateway and their Markdown, media and button capabilities. Credentials and context tokens are omitted; arbitrary recipients cannot be supplied.
- `send_channel_message`: send `text`, `markdown`, `image`, `audio`, `video` or `file`; media may use `auto` to infer the kind. Text uses `text`; media requires exactly one `path` or `attachment_id`, with `text` as its optional caption.
- `send_channel_file`: retained generic media entry point, sharing preparation, immutable snapshots, approval and transport with the message tool.

Channel tasks default to the originating conversation. Other entry points must specify a `target` returned by discovery. IDs last only for the current channel runtime; stop, unbind or restart requires receiving another message and discovering again. TUI/Web/service must already be running the gateway. Standalone MCP/A2A does not start it implicitly, but can access its same-configuration, same-UID local IPC. Platform reply windows, quotas and upload limits still apply.

MCP invokes the registered device tools through `nl2sh_invoke`. A2A `SendMessage` delegates to the Agent using those same tools, without another sending protocol. Discover targets, then send:

```json
{"tool":"send_channel_message","arguments":{"target":"ID returned by discovery","kind":"markdown","text":"## Diagnosis\n\n**Checks complete**"}}
```

Send a file:

```json
{"tool":"send_channel_message","arguments":{"target":"ID returned by discovery","kind":"file","path":"/data/local/tmp/report.txt","text":"Diagnostic report"}}
```

QQ text/Markdown may include up to five command buttons, for example `buttons: [{"label":"Continue","command":"Inspect memory without changes"}]`. Clicking enters normal conversation intake and never directly grants approval. WeChat and media reject buttons. The system generates approval callbacks separately; model-created buttons cannot forge approval authority; generic command buttons reject `/approve`, `/deny` and `/answer`.

`attachment_id` is usable only inside its originating channel Agent task, not across MCP calls, A2A tasks or conversations. External callers should provide local paths. Files are limited to 20 MiB each; preparation fixes bytes and SHA-256, and execution does not reopen the path. Success reports `submitted` (accepted by the platform), not recipient read status. QQ text includes returned message IDs; media and WeChat do not invent unavailable IDs. Partial failures are not automatically retried.

External MCP/A2A calls cannot inherit the target conversation's `/approve session` or permanent channel grants. Independent protocol approval settings still apply. Channel-originated sending retains its own conversation approval.

Channel approval messages, including the second confirmation for dangerous operations, use Markdown: compatible output for WeChat and native Markdown for QQ, retaining ordinary approval buttons. Complete operation previews and reply commands appear in code blocks; embedded Markdown fences cannot hide the preview, and split messages keep code blocks closed. The second confirmation still requires typing the complete confirmation phrase.

Buttons use general QQ platform click permissions instead of treating message-event openids as keyboard UserIDs, avoiding client-side permission rejection. The server strictly binds approval authority to the original requester, bot account, conversation and expiry; other members cannot approve operations.

`/approve session` covers all ordinary operations in this conversation, regardless of changes to file fingerprints, paths, captions or media kinds. New conversations, restarts and high-risk/Root operations still require approval.

Approval commands belong to both WeChat and QQ channels. Reply with `/approve`, `/approve session`, `/approve always` or `/deny` as a standalone message after a pending prompt. Mentioning a command in a message such as “test /approve always” discusses the workflow; it does not grant approval and must not trigger configuration changes. `execute_confirm_policy="always"` means asking every time, not allowing an operation permanently. Session grants and persisted permanent grants receive a localized Markdown acknowledgement; unmatched replies explain the workflow. Dangerous second confirmation must still complete before a grant is saved.

An owner can revoke one permanent grant through the existing protected Web endpoint: `POST /api/channels` with `{"action":"revoke_permanent_approval","id":"<64-character SHA-256 approval ID>"}`. Same-UID channel management IPC accepts the same action. The ID is shown in the approval scope or calculated from the corresponding serialized pattern in the private allowlist. The owner updates memory and disk together, preserving other grants, credentials and configuration; do not edit a running owner's accounts.json directly. This endpoint cannot approve pending requests.
