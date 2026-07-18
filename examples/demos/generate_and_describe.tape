Output examples/demos/generate_and_describe.gif
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

Type "ulx run generate_and_describe.ulx GenerateAndDescribe --arg prompt='a lighthouse at sunset' --run-id gad-vhs-demo --no-cache"
Enter
Wait+Line@180s /\$$/
Sleep 3s
