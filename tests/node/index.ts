import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://localhost:8080/',
  apiKey: 'ollama',
})


const stream = await client.chat.completions.create({
	messages: [{ role: 'user', content: 'Say this is a test' }],
	model: "test",
	stream: true
});

for await (const chunk of stream) {
	console.log(chunk);
} 

//console.log(chatCompletion)

