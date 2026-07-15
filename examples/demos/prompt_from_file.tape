Output examples/demos/prompt_from_file.gif
Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 650
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 40ms
Set Framerate 24

Type "cd examples"
Enter
Sleep 500ms

Type "ulx run prompt_from_file.ulx Greet --arg name=Ada --arg occasion=birthday --provider anthropic --no-cache"
Enter
Sleep 8s
