Output examples/demos/custom_provider.gif
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

Type "ulx run custom_provider.ulx Greet --arg name=World --no-cache"
Enter
Wait+Line@90s /\$$/
Sleep 3s
