Output examples/demos/eval_translate.gif
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

Type "ulx bench eval_translate.ulx TranslateQuality --provider anthropic"
Enter
Wait+Line@240s /\$$/
Sleep 3s
