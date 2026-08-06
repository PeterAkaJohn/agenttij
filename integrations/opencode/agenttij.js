// Reports opencode's state to the agenttij Zellij sidebar.
//
// Install by copying this file to ~/.config/opencode/plugins/agenttij.js
// (global) or .opencode/plugins/agenttij.js (one project).
//
// It shells out to the same tool-agnostic hook Claude Code uses: the script
// only wants a state word and the ZELLIJ_* environment variables that every
// Zellij pane carries, so anything that can run a command on an event can feed
// the sidebar.

const HOOK = `${process.env.HOME}/.claude/hooks/agenttij-state.sh`;

// opencode event -> sidebar state.
//
// `tool.execute.before` stands in for "working": opencode has no
// prompt-submitted event, and `message.updated` fires far too often to spend a
// file write on. The cost is that a turn which calls no tools stays on its
// previous state until `session.idle` reports it done.
//
// `session.idle` is the important one — it means the turn finished and the agent
// is waiting for you, which is what `done` means here.
const STATES = {
  "session.created": "idle",
  "tool.execute.before": "running",
  "permission.asked": "needs-input",
  "session.idle": "done",
  "session.deleted": "gone",
};

export const Agenttij = async ({ $ }) => {
  return {
    event: async ({ event }) => {
      const state = STATES[event.type];
      if (!state) return;

      // Never let reporting break the agent it reports on.
      await $`sh ${HOOK} ${state}`.quiet().nothrow();
    },
  };
};
