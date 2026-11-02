---
name: delegate
description: Use when the user asks to hand a task to another agent, run work in parallel, or "delegate" something, while inside Zellij with the agenttij sidebar - starts a second agent in its own group and git worktree, then waits for it and reads its result.
argument-hint: <task for the other agent>
---

# Hand a task to another agent

The task to hand off: $ARGUMENTS
If that is empty, it is what the user just asked you to delegate.

The other agent runs in a new group of the sidebar, in a fresh git worktree, so it never writes over your files or the user's.
It does not share your conversation: everything it needs has to be in its prompt.

1. **Write the brief.**
   Self-contained: the goal, the files and commands that matter, constraints, what "done" means, and what to say in its final message (a short summary of what it changed and anything left open).
   Keep it to what that agent needs, not your whole context.

2. **Start it, parked.**
   Call `new_group` with `worktree: true` (in a git repository; without one, leave it out) and `run` set to the agent's command with the brief as its prompt - `claude '<brief>'` unless the user named another agent.
   `run` goes through `sh`, so single-quote the brief and write each `'` inside it as `'\''`.
   Do not pass `show`.
   With the `agenttij` command instead: `agenttij group new --worktree --run "claude '<brief>'"`.

3. **Tell the user** what you started and its group id, and that it is parked: they can look at it in the sidebar whenever they like.

4. **Wait, if the user wants the result now.**
   Call `wait_group` with the group id; on a timeout, call it again.
   - `needs-input`: the other agent is asking something.
     Read its pane (`read_pane`, the group id is its first pane) and tell the user what it asks.
     You cannot answer for it: the user answers there, or with `R` in the sidebar.
   - `done` or `unread`: read the end of its pane with `read_pane` and `lines: 80`, and report what it says.

5. **Leave the group open.**
   Its work is in the worktree, waiting for the user to review it; closing the group would let the worktree go.
   Say where it is: `list_groups` shows the group's directory.
