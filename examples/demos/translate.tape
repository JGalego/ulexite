Output examples/demos/translate.gif
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

Type "ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic --no-cache"
Enter
Sleep 10s
