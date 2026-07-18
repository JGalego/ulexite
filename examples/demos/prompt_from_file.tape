Output examples/demos/prompt_from_file.gif
Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 650
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 40ms
Set Framerate 24

Hide
Type "cd examples"
Enter
Type "export PS1='$ '"
Enter
Type "clear"
Enter
Show
Sleep 500ms

Type "ulx run prompt_from_file.ulx Greet --arg name=Ada --arg occasion=birthday --provider anthropic --no-cache"
Enter
Wait+Line@90s /\$$/
Sleep 3s
