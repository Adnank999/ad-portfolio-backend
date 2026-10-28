# Contact form email with Resend

The frontend submits name, email, subject, message, a retry UUID and an empty spam-trap field. Its existing TanStack POST server function checks the request origin and forwards JSON to `POST /api/v1/contact` on the private Rust API. Rust calls `https://api.resend.com/emails` using its existing `reqwest` client dependency. SMTP password recovery remains independent. No database migration is required.

## Production configuration

1. Add and verify your sending domain in [Resend](https://resend.com/docs/dashboard/domains/introduction), including the DNS records shown in its dashboard. Create an API key with sending access for that domain.
2. On the VPS, edit `/root/apps/ad-portfolio-backend/.env.portfolio` and add:

   ```dotenv
   RESEND_API_KEY=re_REPLACE_WITH_YOUR_KEY
   RESEND_FROM_EMAIL=Portfolio <contact@your-verified-domain.example>
   CONTACT_TO_EMAIL=your-real-inbox@example.com
   ```

   Replace all placeholders. `RESEND_FROM_EMAIL` is a sender on your verified domain, not the client's email. `CONTACT_TO_EMAIL` is the inbox that receives enquiries. Visitor email goes into `reply_to`, so replying in your mailbox addresses the client. Restrict this file to the deployment account. Do not commit it or put the key in the frontend, `VITE_*` variables, or browser code. Local Rust runs use `Backend/.env.portfolio` or an explicit `PORTFOLIO_ENV_FILE`.
3. Deploy the backend and frontend through the existing workflows. Compose already reads `.env.portfolio`; the API must be **recreated**, not merely restarted, to pick up changed container environment. The regular API-only deployment does this without recreating PostgreSQL. Do not run `compose down` or touch volumes.
4. Keep frontend runtime `PORTFOLIO_API_URL=http://127.0.0.1:8080` and `APP_ORIGIN=https://adnanhk.dev` (use your actual origin locally). No Resend variables are needed in the frontend. Optional `VITE_CONTACT_EMAIL` only displays a public alternate contact link.
5. Submit one enquiry from `/contact`, check the message status in Resend and the receiving inbox/spam folder, then reply to verify Reply-To. This is the real delivery smoke test; local automated tests use a mock provider and never send real mail.

## Behavior and limits

- Fixed server-owned sender/recipient; no visitor-controlled destinations, HTML, attachments, CC or BCC.
- Required fields validated both at SSR and API boundaries. Unknown JSON fields are rejected; body capped at 24 KiB; message capped at 4,000 characters.
- Honeypot, 3 attempts per normalized sender email per minute, 20 total per minute, and 2 concurrent provider requests per API process. Counters include failed provider attempts, reset on process restart, and are not a distributed abuse-prevention service. For higher traffic/multiple instances, enforce additional limits at the ingress. No untrusted forwarding headers are used as identity.
- A 10-second provider timeout and no automatic retries. The browser retains text and retries unchanged content with the same `contact/<UUID>` Resend idempotency header. Resend retains keys for [24 hours](https://resend.com/docs/dashboard/emails/idempotency-keys). A reload/new enquiry may generate a new key.
- Success requires a 2xx Resend response and valid email ID. It means accepted for sending, not confirmed inbox delivery. No email contents, keys or raw provider error bodies are logged. Monitor delivery/bounces in Resend.
- Missing key: contact returns 503 safely; other features keep working. When a key is set, sender and recipient settings must also be valid. Errors retain entered values and offer a retry. There is no persistent enquiry queue or database copy.
- Disable contact sending by removing/emptying `RESEND_API_KEY` and recreating only the API container. Existing content and mail history are unaffected.

API reference: [Resend send email](https://resend.com/docs/api-reference/emails/send-email).
