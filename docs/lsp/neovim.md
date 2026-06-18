# Neovim — `navigator-lsp`

Neovim 0.11+ ships `vim.lsp.start` so no plugin is required.

## Minimal config

Drop this into `~/.config/nvim/lua/navigator_lsp.lua` and `:luafile` it inside a markdown buffer, or autoload from your
`init.lua`:

```lua
local function start_navigator_lsp()
  local found = vim.fs.find({ ".git" }, { upward = true })
  local root = found[1] and vim.fs.dirname(found[1]) or vim.fn.getcwd()
  vim.lsp.start({
    name = "navigator-lsp",
    cmd = { "navigator-lsp" },
    -- Pin to a workspace root so multiple files share one server.
    root_dir = root,
  })
end

vim.api.nvim_create_autocmd("FileType", {
  pattern = "markdown",
  callback = start_navigator_lsp,
})
```

## Fix-on-save

Wire `source.fixAll` to run automatically:

```lua
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = "*.md",
  callback = function()
    vim.lsp.buf.code_action({
      context = { only = { "source.fixAll" }, diagnostics = {} },
      apply = true,
    })
  end,
})
```

## Manual triggers

- `:lua vim.diagnostic.open_float()` — show the violation under the cursor.
- `:lua vim.lsp.buf.code_action()` — pick a quick-fix from the list.
- `:lua vim.lsp.buf.hover()` — hover bubble for the rule under the cursor.
