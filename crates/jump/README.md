## Overview

- Not a specific feature for Vim mode, can be used in any mode
- Jump input bar as modal, using toolbar pushes down editor which breaks flow
- Simple case insensitive naive substring search within open buffers
- Create jump labels for matches and draw in editor
- Jump labels only single chars with lower and upper case chars
- Jump label priority based on distance to current cursor position and active editor: Home row, top row, bottom row
- Jump to first char of match when label typed

## Configuration

Vim mode use 's' to trigger jump mode.

```json
{
    "context": "Editor && VimControl && !VimWaiting && !menu",
    "bindings": {
      "s": "jump::Toggle",
      ...
    }
}
```

Not implemented:
- Vim motion integration
- Treesitter integration like flash.nvim S shortcut
