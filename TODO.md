## Approval Formatting Improvements

Mixtape is currently showing the details for the tool **after** I've approved.

🛠️  outlook_email_search

Permission required:
y  approve once
e  trust this exact call (session)
t  trust entire tool (session)
n  deny

Choice: y
✓ Approved once
▄▅▅▆▅▅▅▄ thinking
🛠️  outlook_email_search
└ outlook_email_search
endDate: "2026-01-05"
limit: 50
offset: 65
query: "*"
startDate: "2026-01-05"
(user approved)

I'd like to have the tool call show the prompt for approval, then change inline to indicate approval.
We shouldn't see the tool header twice (once for approval and once for execution is how it looks to
be working currently).

## Noisy Tool Output:

I thought we were supposed to have a CLI presenter system that would:
1. Truncate excessively long tool **inputs** along two dimensions:
    a. Long **values** would be truncated
    b. Excessive numbers of keys in an object would be truncated
2. Allow tools to override their CLI presentation with custom presentation that's cleaner.

Here's what I see happening on a outlook_ MCP server.

```
✓ outlook_email_read
└ ✓
{
"content": [
{
"text": "{\n  \"content\": [\n    {\n      \"type\": \"text\",\n      \"text\": \"{\\\"success\\\":true,\\\"
.... (truncated by me for your benefit)
```

I think this is the tool output... but I'm not seeing the tool input parameters.
Can you examine the state of CLI presentation and see what might be missing?

## Input Too Long for Model

❌ Error: Provider error: Invalid configuration: ValidationException: The model returned the following errors: Input is too long for requested model.



## Session DX Is Odd

When I start a new agent using run_cli, its nice that pressing up will bring prior messages from my earlier session in
the directory... it's not so nice that my context window is already taken up by those messages. How can we improve the
DX of this process?


## MCP Server Initialization Failures

Figma Internal Server fails every time on Flexbot agent.