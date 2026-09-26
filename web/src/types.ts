export type EntryKind='user'|'assistant'|'stream'|'tool_call'|'tool_result'|'tool_error'|'tool_output'|'error'|'notice';
export interface Entry{kind:EntryKind;text:string}
export interface Pending{kind:'approval'|'questions';command?:string;risk?:string;explanation?:string;root?:boolean;strong?:boolean;questions?:Question[]}
export interface Question{id:string;header:string;prompt:string;options:{label:string;value:string;description:string}[]}
export interface Session{id:string;title:string;turns:number;busy:boolean;pending:boolean;updated:number}
export interface Snapshot{id:string;title:string;entries:Entry[];busy:boolean;pending:Pending|null;turns:number;steps:number;tool_calls:number;input_tokens:number;output_tokens:number;final_input_tokens?:number;activity:'idle'|'thinking'|'tool'|'waiting';activity_detail?:string|null;activity_elapsed_ms:number;received_at_ms?:number}
export interface QuickSettings{endpoint:string;model:string;provider_ready:boolean;confirm_policy:'always'|'risk_only'|'never';max_context_turns:number;max_agent_steps:number;max_tool_calls:number;context_window?:number;root:string}
export interface ToolInfo{name:string;description:string}
