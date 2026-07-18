Output examples/demos/pdf_qa.gif
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

Type "ulx run pdf_qa.ulx PdfQA --arg doc=fixtures/sample.pdf --arg question='Who rescues the grandmother?' --no-cache"
Enter
Wait+Line@90s /\$$/
Sleep 3s
