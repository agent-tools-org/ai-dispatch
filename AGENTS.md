# Source layout

Source modules live under `src/{cli,cmd,agent,config,cost,prompt,routing,runtime,store,task,ui,workspace,support}/`.
`ui` groups board, tui, web (feature gated), and output; `cli` owns dispatch and diagnostics.
Batch and core types retain their existing root files and child directories.
