# Idea

Drop your raw notes here — brain dump, bullet points, **voice transcription** (superwhisper, audio notes), anything.
This file is the input for `/seed`.

I'd like to create a plugin for the terminal software herdr (https://herdr.dev/) to handle simple lists of todos. This plugin would open a side panel with the lists available for the user. Each list would have a title and a list of draggable and sortable elements, with a similar UI to the todos of basecamp (in the references folder). I would also like an input at the top to add random todos, that when pressing return would get automatically be reordered in the right list, if present, otherwise would be put in a general list. This last part is a feature that should be handled with a software like Jev or Laya (https://github.com/NandhaKishorM/laya), with defined options.
I would also consider using ratatui to be able to add special nice TUI effects (https://ratatui.rs/).
The focus of the plugin is that it should be immediate, simple, reliable. I was thinking to make it work simply by using a TODOS.md file in the folder, that if present, it uses that as the source of the lists; so each list and item would be in that simple markdown file. This would make it easy to use different todo files depending on the opened folder.